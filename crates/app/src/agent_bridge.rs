//! Loopback transport for the agent contract in `ai::agent`.
//!
//! An external agent (Claude Code, via the `qrate-live-review` skill) POSTs one `Request` as JSON
//! and gets one `Response` back. The composition root owns it because it is the only place that
//! links both the contract and the live table. Every call is filed with the Agent panel on its way
//! out, refusals included — this is the only place that sees all of them.
//!
//! Off unless `QRATE_AGENT_BRIDGE=1`: a port that hands out the open project's contents is not
//! something to run by default. When on, it binds 127.0.0.1 on an ephemeral port and writes the
//! port plus a per-run bearer token to `agent-bridge.json` beside the log, so anything that can
//! read the answers already has the user's own file permissions.
//!
//! The accept loop is a foreground poll rather than a thread because answering means reading live
//! GPUI state, which only the main thread may do.

use std::collections::HashMap;
use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{App, SharedString};
use serde_json::json;
use workspace::{AgentCall, AgentEntry};

const POLL: Duration = Duration::from_millis(250);
/// How long an authenticated agent may go quiet before the panel calls it disconnected. The
/// protocol has no session and no goodbye — one request, one answer, socket closed — so silence is
/// the only signal there is.
const IDLE: Duration = Duration::from_secs(60);
/// A name the caller chose, never proof of anything. Anything holding the token can claim any name.
const AGENT_HEADER: &str = "x-agent";
const UNNAMED: &str = "unnamed agent";
/// A panel row, not a paragraph.
const MAX_AGENT_NAME: usize = 48;
/// Enough for the largest `Request` the contract allows — a full batch of staged findings, each
/// carrying the cell text it was judged against — and nothing like enough to be a spool.
const MAX_BODY: usize = 256 * 1024;

fn endpoint_path() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("agent-bridge.json"))
}

/// Bind the bridge and start answering, if this run opted in.
pub fn init(cx: &mut App) {
    if std::env::var("QRATE_AGENT_BRIDGE").as_deref() != Ok("1") {
        return;
    }

    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("agent bridge could not bind a loopback port, staying off: {err}");
            return;
        }
    };
    if let Err(err) = listener.set_nonblocking(true) {
        log::error!("agent bridge could not poll its listener, staying off: {err}");
        return;
    }

    // ponytail: `RandomState` keys are OS randomness gathered at process start, which is the
    // strongest source std exposes. Swap in `getrandom` if this ever guards more than loopback.
    let token = format!(
        "{:016x}{:016x}",
        RandomState::new().hash_one(0u8),
        RandomState::new().hash_one(1u8)
    );
    let Some(path) = endpoint_path() else {
        log::error!("agent bridge has nowhere to publish its port, staying off");
        return;
    };
    let port = listener.local_addr().map_or(0, |addr| addr.port());
    let endpoint = json!({ "url": format!("http://127.0.0.1:{port}"), "token": token });
    if let Err(err) = fs::write(&path, endpoint.to_string()) {
        log::error!(
            "agent bridge could not write {}, staying off: {err}",
            path.display()
        );
        return;
    }
    log::info!(
        "agent bridge listening on 127.0.0.1:{port}, endpoint in {}",
        path.display()
    );

    cx.spawn(async move |cx| {
        // Last authenticated call per claimed name, owned by the accept loop rather than a global
        // — nothing outside this loop has any use for it.
        let mut seen: HashMap<SharedString, Instant> = HashMap::new();
        loop {
            cx.background_executor().timer(POLL).await;
            loop {
                match listener.accept() {
                    Ok((stream, _)) => cx.update(|cx| serve(stream, &token, &mut seen, cx)),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(err) => {
                        log::warn!("agent bridge dropped a connection: {err}");
                        break;
                    }
                }
            }
            let gone: Vec<SharedString> = seen
                .iter()
                .filter(|(_, last)| last.elapsed() >= IDLE)
                .map(|(agent, _)| agent.clone())
                .collect();
            for agent in gone {
                seen.remove(&agent);
                cx.update(|cx| {
                    workspace::record_agent_call(
                        lifecycle(agent, "disconnected", "quiet for a minute"),
                        cx,
                    )
                });
            }
        }
    })
    .detach();
}

/// A connect or disconnect, which the transport infers because the protocol carries no session.
fn lifecycle(agent: SharedString, label: &'static str, why: &'static str) -> AgentCall {
    AgentCall {
        agent,
        label: label.into(),
        detail: SharedString::default(),
        outcome: why.into(),
        entry: AgentEntry::Lifecycle,
        took: Duration::ZERO,
    }
}

/// Delete the published endpoint so nothing points at a port this app no longer owns.
pub fn shutdown() {
    if let Some(path) = endpoint_path() {
        let _ = fs::remove_file(path);
    }
}

fn serve(
    mut stream: TcpStream,
    token: &str,
    seen: &mut HashMap<SharedString, Instant>,
    cx: &mut App,
) {
    let framed = stream
        .set_read_timeout(Some(POLL))
        .and_then(|()| stream.set_write_timeout(Some(POLL)))
        .and_then(|()| stream.set_nonblocking(false));
    if let Err(err) = framed {
        log::warn!("agent bridge could not configure a connection: {err}");
        return;
    }

    let started = Instant::now();
    let parsed = stream
        .try_clone()
        .map_err(|_| "unreadable_request")
        .and_then(|clone| read_request(&mut BufReader::new(clone)));

    // A request that never parsed has no method to name, and that is itself worth showing.
    let (mut method, mut detail) = (SharedString::from("—"), SharedString::default());
    let agent = parsed
        .as_ref()
        .ok()
        .and_then(|request| request.agent.clone())
        .map_or_else(|| SharedString::from(UNNAMED), SharedString::from);

    // Only an authenticated call counts as connecting. A refused one still gets an entry, but
    // letting it into `seen` would let anything on loopback grow the map with invented names.
    let authenticated = parsed
        .as_ref()
        .is_ok_and(|request| request.credential.as_deref() == Some(token));
    if authenticated && !seen.contains_key(&agent) {
        workspace::record_agent_call(
            lifecycle(agent.clone(), "connected", "first call from this agent"),
            cx,
        );
    }
    if authenticated {
        seen.insert(agent.clone(), Instant::now());
    }

    let (status, payload, outcome, refused) = match parsed {
        Err(reason) => (
            "400 Bad Request",
            json!({ "error": reason }),
            reason.into(),
            true,
        ),
        Ok(request) if request.credential.as_deref() != Some(token) => {
            log::warn!("agent bridge refused a request with a missing or wrong token");
            (
                "403 Forbidden",
                json!({ "error": "forbidden" }),
                SharedString::from("forbidden"),
                true,
            )
        }
        Ok(request) => match serde_json::from_slice::<ai::agent::Request>(&request.body) {
            Err(err) => (
                "400 Bad Request",
                json!({ "error": "malformed_request", "detail": err.to_string() }),
                SharedString::from("malformed_request"),
                true,
            ),
            Ok(request) => {
                (method, detail) = summarize(&request);
                match table::respond_to_agent(request, cx) {
                    Ok(response) => {
                        let outcome = describe(&response.result);
                        (
                            "200 OK",
                            serde_json::to_value(response).unwrap_or_else(|_| json!({})),
                            outcome,
                            false,
                        )
                    }
                    Err(error) => (
                        "400 Bad Request",
                        json!({ "error": error }),
                        wire_name(&error),
                        true,
                    ),
                }
            }
        },
    };

    workspace::record_agent_call(
        AgentCall {
            agent,
            label: method,
            detail,
            outcome,
            entry: match refused {
                true => AgentEntry::Refused,
                false => AgentEntry::Answered,
            },
            took: started.elapsed(),
        },
        cx,
    );

    let body = payload.to_string();
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Err(err) = stream.write_all(reply.as_bytes()) {
        log::warn!("agent bridge could not answer a connection: {err}");
    }
}

/// The method name and a one-line summary of what was asked for.
///
/// The name is read back out of the serialized request rather than matched by hand, so the panel
/// can never disagree with what actually went over the wire.
fn summarize(request: &ai::agent::Request) -> (SharedString, SharedString) {
    let method = serde_json::to_value(request)
        .ok()
        .and_then(|value| value["method"].as_str().map(SharedString::from))
        .unwrap_or_else(|| "?".into());

    let detail = match request {
        ai::agent::Request::Rows { rows } => format!("{} row(s)", rows.len()).into(),
        ai::agent::Request::SearchRows { query, limit } => format!("“{query}”, max {limit}").into(),
        ai::agent::Request::StageFindings { revision, findings } => {
            format!("{} finding(s) at revision {}", findings.len(), revision.0).into()
        }
        _ => SharedString::default(),
    };
    (method, detail)
}

/// What an answer amounted to — the size of it, not the contents. The panel is a record that a
/// call happened; the archive's own data does not belong in a debugging list.
fn describe(result: &ai::agent::ResultSet) -> SharedString {
    match result {
        ai::agent::ResultSet::ProjectSummary(summary) => format!(
            "{} rows × {} columns",
            summary.row_count, summary.column_count
        )
        .into(),
        ai::agent::ResultSet::Columns { columns } => format!("{} columns", columns.len()).into(),
        ai::agent::ResultSet::Rows { rows } => format!("{} rows", rows.len()).into(),
        ai::agent::ResultSet::Diagnostics { diagnostics } => {
            format!("{} diagnostics", diagnostics.len()).into()
        }
        ai::agent::ResultSet::SelectedRows { rows } => format!("{} selected", rows.len()).into(),
        ai::agent::ResultSet::Staged { accepted, stale } => {
            format!("{accepted} staged, {} stale", stale.len()).into()
        }
    }
}

/// A refusal under the name the agent was given for it, so the panel and the reply agree.
fn wire_name(error: &ai::agent::RequestError) -> SharedString {
    serde_json::to_value(error)
        .ok()
        .and_then(|value| value.as_str().map(SharedString::from))
        .unwrap_or_else(|| "request_error".into())
}

struct RawRequest {
    /// The bearer token as sent, or `None` for anything that is not a bearer credential.
    credential: Option<String>,
    /// What the caller named itself in `X-Agent`. A label, not a claim the bridge can check.
    agent: Option<String>,
    body: Vec<u8>,
}

/// Read one HTTP request, far enough to know who is asking and what they sent.
///
/// The method and path are ignored on purpose — this speaks HTTP only so that a `curl` an agent
/// already knows how to write is a working client.
fn read_request(reader: &mut impl BufRead) -> Result<RawRequest, &'static str> {
    let mut credential = None;
    let mut agent = None;
    let mut length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return Err("unreadable_request"),
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = header(line, "authorization") {
            credential = value.strip_prefix("Bearer ").map(str::to_string);
        }
        if let Some(value) = header(line, "content-length") {
            length = value.parse().map_err(|_| "malformed_request")?;
        }
        // Truncated rather than refused: a silly name is not worth failing a call over, but an
        // unbounded one would let a caller write an essay into every row of the panel.
        if let Some(value) = header(line, AGENT_HEADER) {
            agent = Some(value.chars().take(MAX_AGENT_NAME).collect());
        }
    }
    if length > MAX_BODY {
        return Err("request_too_large");
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| "truncated_request")?;
    Ok(RawRequest {
        credential,
        agent,
        body,
    })
}

fn header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    key.eq_ignore_ascii_case(name).then(|| value.trim())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MAX_AGENT_NAME, MAX_BODY, read_request};

    fn request(headers: &str, body: &str) -> Cursor<Vec<u8>> {
        Cursor::new(
            format!(
                "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        )
    }

    fn credential(headers: &str) -> Option<String> {
        read_request(&mut request(headers, "{}"))
            .ok()
            .and_then(|parsed| parsed.credential)
    }

    /// Nothing but a bearer credential may become a token — otherwise an unauthenticated request
    /// reaches the live table.
    #[test]
    fn only_a_bearer_credential_counts_as_a_token() {
        assert_eq!(credential(""), None);
        assert_eq!(credential("Authorization: Basic abc\r\n"), None);
        assert_eq!(credential("Authorization: Bearer\r\n"), None);
        assert_eq!(
            credential("authorization: Bearer s3cret\r\n"),
            Some("s3cret".into())
        );
    }

    /// The panel's identity column. Optional, so a caller that sends nothing still works, and
    /// bounded, so no caller can write a paragraph into every row of the panel.
    #[test]
    fn an_agent_may_name_itself_within_bounds() {
        let named = |headers: &str| {
            read_request(&mut request(headers, "{}"))
                .ok()
                .and_then(|parsed| parsed.agent)
        };
        assert_eq!(named(""), None);
        assert_eq!(
            named("X-Agent: claude-code\r\n"),
            Some("claude-code".into())
        );
        assert_eq!(
            named(&format!("x-agent: {}\r\n", "n".repeat(MAX_AGENT_NAME + 20)))
                .map(|name| name.len()),
            Some(MAX_AGENT_NAME)
        );
    }

    #[test]
    fn the_body_survives_header_casing() {
        let parsed = read_request(&mut request(
            "authorization: Bearer s3cret\r\n",
            "{\"a\":1}",
        ))
        .unwrap();
        assert_eq!(parsed.body, b"{\"a\":1}");
    }

    #[test]
    fn an_oversized_body_is_refused_before_it_is_read() {
        let mut oversized = Cursor::new(
            format!(
                "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            )
            .into_bytes(),
        );
        assert_eq!(
            read_request(&mut oversized).err(),
            Some("request_too_large")
        );
    }
}
