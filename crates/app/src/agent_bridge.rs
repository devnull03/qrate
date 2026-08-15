//! Loopback transport for the read-only agent contract in `ai::agent`.
//!
//! An external agent (Claude Code, via the `qrate-live-review` skill) POSTs one `Request` as JSON
//! and gets one `Response` back. The composition root owns it because it is the only place that
//! links both the contract and the live table.
//!
//! Off unless `QRATE_AGENT_BRIDGE=1`: a port that hands out the open project's contents is not
//! something to run by default. When on, it binds 127.0.0.1 on an ephemeral port and writes the
//! port plus a per-run bearer token to `agent-bridge.json` beside the log, so anything that can
//! read the answers already has the user's own file permissions.
//!
//! The accept loop is a foreground poll rather than a thread because answering means reading live
//! GPUI state, which only the main thread may do.

use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use gpui::App;
use serde_json::json;

const POLL: Duration = Duration::from_millis(250);
/// Enough for the largest `Request` the contract allows, and nothing like enough to be a spool.
const MAX_BODY: usize = 16 * 1024;

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
        loop {
            cx.background_executor().timer(POLL).await;
            loop {
                match listener.accept() {
                    Ok((stream, _)) => cx.update(|cx| serve(stream, &token, cx)),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(err) => {
                        log::warn!("agent bridge dropped a connection: {err}");
                        break;
                    }
                }
            }
        }
    })
    .detach();
}

/// Delete the published endpoint so nothing points at a port this app no longer owns.
pub fn shutdown() {
    if let Some(path) = endpoint_path() {
        let _ = fs::remove_file(path);
    }
}

fn serve(mut stream: TcpStream, token: &str, cx: &App) {
    let framed = stream
        .set_read_timeout(Some(POLL))
        .and_then(|()| stream.set_write_timeout(Some(POLL)))
        .and_then(|()| stream.set_nonblocking(false));
    if let Err(err) = framed {
        log::warn!("agent bridge could not configure a connection: {err}");
        return;
    }

    let parsed = stream
        .try_clone()
        .map_err(|_| "unreadable_request")
        .and_then(|clone| read_request(&mut BufReader::new(clone)));

    let (status, payload) = match parsed {
        Err(reason) => ("400 Bad Request", json!({ "error": reason })),
        Ok(request) if request.credential.as_deref() != Some(token) => {
            log::warn!("agent bridge refused a request with a missing or wrong token");
            ("403 Forbidden", json!({ "error": "forbidden" }))
        }
        Ok(request) => match serde_json::from_slice::<ai::agent::Request>(&request.body) {
            Err(err) => (
                "400 Bad Request",
                json!({ "error": "malformed_request", "detail": err.to_string() }),
            ),
            Ok(request) => match table::respond_to_agent(request, cx) {
                Ok(response) => (
                    "200 OK",
                    serde_json::to_value(response).unwrap_or_else(|_| json!({})),
                ),
                Err(error) => ("400 Bad Request", json!({ "error": error })),
            },
        },
    };

    let body = payload.to_string();
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Err(err) = stream.write_all(reply.as_bytes()) {
        log::warn!("agent bridge could not answer a connection: {err}");
    }
}

struct RawRequest {
    /// The bearer token as sent, or `None` for anything that is not a bearer credential.
    credential: Option<String>,
    body: Vec<u8>,
}

/// Read one HTTP request, far enough to know who is asking and what they sent.
///
/// The method and path are ignored on purpose — this speaks HTTP only so that a `curl` an agent
/// already knows how to write is a working client.
fn read_request(reader: &mut impl BufRead) -> Result<RawRequest, &'static str> {
    let mut credential = None;
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
    }
    if length > MAX_BODY {
        return Err("request_too_large");
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| "truncated_request")?;
    Ok(RawRequest { credential, body })
}

fn header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    key.eq_ignore_ascii_case(name).then(|| value.trim())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MAX_BODY, read_request};

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
