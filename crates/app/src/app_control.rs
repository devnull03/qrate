//! The desktop's one private endpoint. Only a `qrate-cli` from the same release talks to it; the
//! public contract for scripts and agents is the CLI's JSON, not this.
//!
//! Loopback TCP behind a per-launch token published in `app-control.json`. Sockets are read and
//! written on their own threads; the main thread only answers, because only it may read GPUI state.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gpui::{App, SharedString};
use serde::Serialize;
use serde_json::{Value, json};
use workspace::{AgentCall, AgentEntry};

const PROTOCOL: u8 = 1;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Enough for a staged rewrite of a whole column of long values.
const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_AGENT_NAME: usize = 48;
const MAX_LINE: u64 = 8 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_CONNECTIONS: usize = 16;
const MAX_QUEUED_JOBS: usize = 32;
const MAX_AGENT_NAMES: usize = 256;
const UNNAMED: &str = "unnamed agent";

/// Whether agents may use the live project. Absent means on; status and project info ignore it.
pub const AGENT_ACCESS_KEY: &str = "agent_access";

fn endpoint_path() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("app-control.json"))
}

enum Call {
    Status,
    ProjectInfo,
    Agent { agent: SharedString, body: Vec<u8> },
}

struct Job {
    call: Call,
    reply: async_channel::Sender<(&'static str, Value)>,
}

pub fn init(cx: &mut App) {
    // Written once so the Settings switch shows the default rather than reading "off".
    if !settings::AppSettings::get(cx)
        .values
        .contains_key(AGENT_ACCESS_KEY)
    {
        settings::AppSettings::set_bool(AGENT_ACCESS_KEY, true, cx);
    }
    let Some((listener, token)) = start() else {
        return;
    };
    let (jobs, inbox) = async_channel::bounded::<Job>(MAX_QUEUED_JOBS);
    let spawned = std::thread::Builder::new()
        .name("app-control".into())
        .spawn(move || {
            let open = Arc::new(AtomicUsize::new(0));
            for stream in listener.incoming() {
                let (token, jobs) = (token.clone(), jobs.clone());
                match stream {
                    Ok(_) if open.load(Ordering::Acquire) >= MAX_CONNECTIONS => {
                        log::warn!("app control refused a connection: too many already open");
                    }
                    Ok(stream) => {
                        open.fetch_add(1, Ordering::AcqRel);
                        let slot = open.clone();
                        let spawned = std::thread::Builder::new()
                            .name("app-control-connection".into())
                            .spawn(move || {
                                serve(stream, &token, &jobs);
                                slot.fetch_sub(1, Ordering::AcqRel);
                            });
                        if spawned.is_err() {
                            open.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    Err(error) => log::warn!("app control dropped a connection: {error}"),
                }
            }
        });
    if let Err(error) = spawned {
        log::error!("app control could not start its listener thread: {error}");
        shutdown();
        return;
    }
    cx.spawn(async move |cx| {
        let seen = Rc::new(RefCell::new(HashSet::new()));
        while let Ok(job) = inbox.recv().await {
            let answer = match job.call {
                Call::Status => cx.update(|cx| ("200 OK", status(cx))),
                Call::ProjectInfo => cx.update(project_info),
                // Each agent call answers on its own, so a delete waiting for the archivist does
                // not hold up every other call behind it.
                Call::Agent { agent, body } => {
                    let seen = seen.clone();
                    cx.spawn(async move |cx| {
                        let answer = answer_agent(agent, &body, &seen, cx).await;
                        let _ = job.reply.send(answer).await;
                    })
                    .detach();
                    continue;
                }
            };
            let _ = job.reply.send(answer).await;
        }
    })
    .detach();
}

fn start() -> Option<(TcpListener, String)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .inspect_err(|error| log::error!("app control could not bind loopback: {error}"))
        .ok()?;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .inspect_err(|error| log::error!("app control could not generate a token: {error}"))
        .ok()?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = endpoint_path().or_else(|| {
        log::error!("app control has no application-data directory to publish into");
        None
    })?;
    let descriptor = json!({
        "app_control_protocol": PROTOCOL,
        "url": format!("http://{}", listener.local_addr().ok()?),
        "token": token,
    });
    write_private(&path, descriptor.to_string().as_bytes())
        .inspect_err(|error| log::error!("app control could not write {}: {error}", path.display()))
        .ok()?;
    log::info!("app control listening, endpoint in {}", path.display());
    Some((listener, token))
}

fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    // Renamed into place so the CLI never reads a half-written descriptor.
    let partial = path.with_extension("json.tmp");
    options.open(&partial)?.write_all(contents)?;
    fs::rename(partial, path)
}

pub fn shutdown() {
    if let Some(path) = endpoint_path() {
        let _ = fs::remove_file(path);
    }
}

fn serve(mut stream: TcpStream, token: &str, jobs: &async_channel::Sender<Job>) {
    let configured = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(IO_TIMEOUT)))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)));
    let request = configured
        .map_err(|_| "unreadable_request")
        .and_then(|()| stream.try_clone().map_err(|_| "unreadable_request"))
        .and_then(|clone| read_request(&mut BufReader::new(clone)));
    let (status, body) = match request {
        Err("request_too_large") => (
            "413 Payload Too Large",
            json!({ "error": "request_too_large" }),
        ),
        Err(reason) => ("400 Bad Request", json!({ "error": reason })),
        Ok(request) if request.credential.as_deref() != Some(token) => {
            log::warn!("app control refused a request with a missing or wrong token");
            ("401 Unauthorized", json!({ "error": "unauthorized" }))
        }
        Ok(request) => {
            let call = match (request.method.as_str(), request.path.as_str()) {
                ("GET", "/v1/status") => Some(Call::Status),
                ("GET", "/v1/project/info") => Some(Call::ProjectInfo),
                ("POST", "/v1/agent") => Some(Call::Agent {
                    agent: request
                        .agent
                        .map_or_else(|| UNNAMED.into(), SharedString::from),
                    body: request.body,
                }),
                _ => None,
            };
            let (reply, answer) = async_channel::bounded(1);
            match call {
                None => ("404 Not Found", json!({ "error": "not_found" })),
                Some(call) => match jobs.send_blocking(Job { call, reply }) {
                    Ok(()) => answer
                        .recv_blocking()
                        .unwrap_or(("503 Service Unavailable", json!({ "error": "app_closing" }))),
                    Err(_) => ("503 Service Unavailable", json!({ "error": "app_closing" })),
                },
            }
        }
    };
    let body = body.to_string();
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Err(error) = stream.write_all(reply.as_bytes()) {
        log::warn!("app control could not answer a connection: {error}");
    }
}

fn status(cx: &mut App) -> Value {
    let project = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|project| project.file.to_string_lossy().into_owned());
    json!({ "app_control_protocol": PROTOCOL, "project": project })
}

#[derive(Serialize)]
struct ProjectInfo {
    name: String,
    path: String,
    source: Option<String>,
    created_at: Option<String>,
    link_method: Option<String>,
    files_folder: Option<String>,
    row_count: usize,
    column_count: usize,
}

fn project_info(cx: &mut App) -> (&'static str, Value) {
    let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
        return ("409 Conflict", json!({ "error": "no_active_project" }));
    };
    let setting = |key: &str| {
        project
            .data
            .values
            .get(key)
            .map(|value| value.text().to_string())
            .filter(|value| !value.is_empty())
    };
    let info = ProjectInfo {
        name: project.display_name(),
        path: project.file.to_string_lossy().into_owned(),
        source: setting("source"),
        created_at: setting("created_at"),
        link_method: setting("link_method"),
        files_folder: setting(settings::project::FILES_FOLDER_KEY),
        row_count: project.data.rows.len(),
        column_count: project.data.columns.len(),
    };
    (
        "200 OK",
        json!({ "app_control_protocol": PROTOCOL, "project": info }),
    )
}

async fn answer_agent(
    agent: SharedString,
    body: &[u8],
    seen: &RefCell<HashSet<SharedString>>,
    cx: &mut gpui::AsyncApp,
) -> (&'static str, Value) {
    let started = Instant::now();
    let allowed = cx.update(|cx| {
        settings::AppSettings::get(cx)
            .values
            .get(AGENT_ACCESS_KEY)
            .is_none_or(|value| value.bool())
    });
    let (mut method, mut detail) = (SharedString::from("—"), SharedString::default());
    let (status, payload, outcome, refused) = match serde_json::from_slice::<WireCall>(body) {
        _ if !allowed => (
            "403 Forbidden",
            json!({ "error": "agent_access_off" }),
            SharedString::from("agent access is off in Settings"),
            true,
        ),
        Err(error) => (
            "400 Bad Request",
            json!({ "error": "malformed_request", "detail": error.to_string() }),
            SharedString::from("malformed_request"),
            true,
        ),
        Ok(call) => {
            let (name, params) = call.resolve();
            method = name.clone().into();
            let dispatched = cx.update(|cx| {
                capability::dispatch(&name, params, capability::Caller::Agent(agent.clone()), cx)
            });
            detail = dispatched.label.into();
            match dispatched.task.await {
                Ok(answer) => ("200 OK", answer.value, answer.outcome.into(), false),
                Err(error) => {
                    let outcome = wire_name(&error);
                    ("400 Bad Request", json!({ "error": error }), outcome, true)
                }
            }
        }
    };
    cx.update(|cx| {
        let mut seen = seen.borrow_mut();
        if seen.len() < MAX_AGENT_NAMES && seen.insert(agent.clone()) {
            workspace::record_agent_call(
                AgentCall {
                    agent: agent.clone(),
                    label: "connected".into(),
                    detail: SharedString::default(),
                    outcome: "first call from this agent".into(),
                    entry: AgentEntry::Lifecycle,
                    took: Duration::ZERO,
                },
                cx,
            );
        }
        workspace::record_agent_call(
            AgentCall {
                agent,
                label: method,
                detail,
                outcome,
                entry: if refused {
                    AgentEntry::Refused
                } else {
                    AgentEntry::Answered
                },
                took: started.elapsed(),
            },
            cx,
        )
    });
    (status, payload)
}

/// One agent call off the wire: a capability's name and its parameters.
#[derive(serde::Deserialize)]
struct WireCall {
    method: String,
    #[serde(default)]
    params: Value,
}

impl WireCall {
    /// The capability to call. `edit` with an `op` is the CLI's older spelling of the shape edits,
    /// kept because the CLI's JSON is a public contract.
    fn resolve(self) -> (String, Value) {
        match (self.method.as_str(), &self.params) {
            ("edit", Value::Object(params)) => {
                let mut params = params.clone();
                match params.remove("op") {
                    Some(Value::String(op)) => (op, Value::Object(params)),
                    _ => ("edit".into(), Value::Object(params)),
                }
            }
            _ => (self.method, self.params),
        }
    }
}

fn wire_name(error: &ai::agent::RequestError) -> SharedString {
    serde_json::to_value(error)
        .ok()
        .and_then(|value| value.get("code")?.as_str().map(SharedString::from))
        .unwrap_or_else(|| "request_error".into())
}

struct RawRequest {
    method: String,
    path: String,
    credential: Option<String>,
    /// What the caller named itself in `X-Agent`. A label, never proof.
    agent: Option<String>,
    body: Vec<u8>,
}

fn read_bounded_line(reader: &mut impl BufRead, line: &mut String) -> Result<usize, &'static str> {
    let read = reader
        .by_ref()
        .take(MAX_LINE)
        .read_line(line)
        .map_err(|_| "unreadable_request")?;
    if read as u64 == MAX_LINE && !line.ends_with('\n') {
        return Err("request_too_large");
    }
    Ok(read)
}

fn read_request(reader: &mut impl BufRead) -> Result<RawRequest, &'static str> {
    let mut line = String::new();
    read_bounded_line(reader, &mut line)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return Err("malformed_request");
    };
    let (method, path) = (method.to_owned(), path.to_owned());
    let (mut credential, mut agent, mut length) = (None, None, 0usize);
    for _ in 0..=MAX_HEADERS {
        line.clear();
        if read_bounded_line(reader, &mut line)? == 0 {
            break;
        }
        let Some((name, value)) = line.trim_end().split_once(':') else {
            break;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            credential = value.strip_prefix("Bearer ").map(str::to_owned);
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().map_err(|_| "malformed_request")?;
        } else if name.eq_ignore_ascii_case("x-agent") {
            agent = Some(value.chars().take(MAX_AGENT_NAME).collect());
        }
    }
    if line.contains(':') || length > MAX_BODY {
        return Err("request_too_large");
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| "truncated_request")?;
    Ok(RawRequest {
        method,
        path,
        credential,
        agent,
        body,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    /// `docs/cli/capabilities.md` is generated from every capability the app registers; this
    /// fails when it is stale. Regenerate it with `QRATE_BLESS=1 cargo test -p app capability_reference`.
    #[gpui::test]
    fn capability_reference_matches_the_catalog(cx: &mut gpui::TestAppContext) {
        let page = cx.update(|cx| {
            capability::init(cx);
            table::init_capabilities(cx);
            capability::register::<checks::authority::search::AuthoritySearchCap>(cx);
            capability::markdown(&capability::catalog(cx))
        });
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cli/capabilities.md");
        if std::env::var_os("QRATE_BLESS").is_some() {
            std::fs::write(&path, &page).unwrap();
        }
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            written == page,
            "docs/cli/capabilities.md is stale: run QRATE_BLESS=1 cargo test -p app capability_reference"
        );
    }

    use super::{MAX_AGENT_NAME, MAX_BODY, MAX_HEADERS, MAX_LINE, read_request};

    fn request(headers: &str, body: &str) -> Cursor<Vec<u8>> {
        Cursor::new(
            format!(
                "POST /v1/agent HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        )
    }

    #[test]
    fn only_a_bearer_credential_counts_as_a_token() {
        let credential = |headers: &str| {
            read_request(&mut request(headers, "{}"))
                .unwrap()
                .credential
        };
        assert_eq!(credential(""), None);
        assert_eq!(credential("Authorization: Basic abc\r\n"), None);
        assert_eq!(credential("Authorization: Bearer\r\n"), None);
        assert_eq!(
            credential("authorization: Bearer s3cret\r\n"),
            Some("s3cret".into())
        );
    }

    #[test]
    fn reads_route_agent_and_body() {
        let parsed = read_request(&mut request(
            &format!("X-Agent: {}\r\n", "n".repeat(MAX_AGENT_NAME + 20)),
            "{\"a\":1}",
        ))
        .unwrap();
        assert_eq!(
            (parsed.method.as_str(), parsed.path.as_str()),
            ("POST", "/v1/agent")
        );
        assert_eq!(parsed.agent.map(|name| name.len()), Some(MAX_AGENT_NAME));
        assert_eq!(parsed.body, b"{\"a\":1}");
    }

    #[test]
    fn an_oversized_body_is_refused_before_it_is_read() {
        let mut oversized = Cursor::new(
            format!(
                "POST /v1/agent HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            )
            .into_bytes(),
        );
        assert_eq!(
            read_request(&mut oversized).err(),
            Some("request_too_large")
        );
    }

    #[test]
    fn oversized_or_endless_headers_are_refused() {
        let long_line = format!("X-Agent: {}\r\n", "n".repeat(MAX_LINE as usize));
        let too_many = "X-Pad: 1\r\n".repeat(MAX_HEADERS + 1);
        for headers in [long_line, too_many] {
            assert_eq!(
                read_request(&mut request(&headers, "{}")).err(),
                Some("request_too_large")
            );
        }
        assert!(read_request(&mut request(&"X-Pad: 1\r\n".repeat(MAX_HEADERS - 3), "{}")).is_ok());
    }
}
