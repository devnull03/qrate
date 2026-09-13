//! Private, read-only local control endpoint for the qrate CLI.

use std::{
    fs,
    hash::{BuildHasher, RandomState},
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::PathBuf,
    time::Duration,
};

use gpui::App;
use serde::Serialize;
use serde_json::json;

const POLL: Duration = Duration::from_millis(250);

fn endpoint_path() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("app-control.json"))
}

pub fn init(cx: &mut App) {
    let Some((listener, token)) = start() else {
        return;
    };
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            loop {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &token, cx),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        log::warn!("app control dropped a connection: {error}");
                        break;
                    }
                }
            }
        }
    })
    .detach();
}

fn start() -> Option<(TcpListener, String)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .inspect_err(|error| log::error!("app control could not bind loopback: {error}"))
        .ok()?;
    listener
        .set_nonblocking(true)
        .inspect_err(|error| log::error!("app control could not poll its listener: {error}"))
        .ok()?;
    let token = format!(
        "{:016x}{:016x}",
        RandomState::new().hash_one(2u8),
        RandomState::new().hash_one(3u8)
    );
    let path = endpoint_path()?;
    let descriptor = json!({
        "app_control_protocol": 1,
        "url": format!("http://{}", listener.local_addr().ok()?),
        "token": token,
    });
    fs::write(&path, descriptor.to_string())
        .inspect_err(|error| log::error!("app control could not write {}: {error}", path.display()))
        .ok()?;
    Some((listener, token))
}

fn serve(mut stream: TcpStream, token: &str, cx: &mut gpui::AsyncApp) {
    let _ = stream
        .set_read_timeout(Some(POLL))
        .and_then(|()| stream.set_write_timeout(Some(POLL)));
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut lines = BufReader::new(clone).lines();
    let request = lines.next().and_then(Result::ok).unwrap_or_default();
    let authenticated = lines
        .map_while(Result::ok)
        .take_while(|line| !line.is_empty())
        .any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("authorization")
                    && value.trim() == format!("Bearer {token}")
            })
        });
    if !authenticated {
        respond(&mut stream, "401 Unauthorized", "{}");
        return;
    }
    match request.as_str() {
        "GET /status HTTP/1.1" => {
            let project = cx.update(|cx| {
                cx.try_global::<settings::project::CurrentProject>()
                    .map(|project| project.file.to_string_lossy().into_owned())
            });
            respond(
                &mut stream,
                "200 OK",
                &json!({ "running": true, "project": project }).to_string(),
            );
        }
        "GET /v1/project/info HTTP/1.1" => {
            let project = cx.update(|cx| {
                cx.try_global::<settings::project::CurrentProject>()
                    .map(project_info)
            });
            match project {
                Some(project) => respond(
                    &mut stream,
                    "200 OK",
                    &serde_json::to_string(&ProjectInfoResponse {
                        app_control_protocol: 1,
                        project,
                    })
                    .expect("project info is serializable"),
                ),
                None => respond(
                    &mut stream,
                    "409 Conflict",
                    r#"{"app_control_protocol":1,"error":"no_active_project"}"#,
                ),
            }
        }
        _ => respond(&mut stream, "404 Not Found", "{}"),
    }
}

#[derive(Serialize)]
struct ProjectInfoResponse {
    app_control_protocol: u8,
    project: ProjectInfo,
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

fn project_info(project: &settings::project::CurrentProject) -> ProjectInfo {
    let setting = |key: &str| {
        project
            .data
            .values
            .get(key)
            .map(|value| value.text().to_string())
            .filter(|value| !value.is_empty())
    };
    ProjectInfo {
        name: project.display_name(),
        path: project.file.to_string_lossy().into_owned(),
        source: setting("source"),
        created_at: setting("created_at"),
        link_method: setting("link_method"),
        files_folder: setting(settings::project::FILES_FOLDER_KEY),
        row_count: project.data.rows.len(),
        column_count: project.data.columns.len(),
    }
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

pub fn shutdown() {
    if let Some(path) = endpoint_path() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{ProjectInfo, ProjectInfoResponse, respond};
    use std::io::Read;

    #[test]
    fn response_is_http_framed() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let (mut stream, _) = listener.accept().unwrap();
        respond(&mut stream, "200 OK", "{}");
        drop(stream);
        assert_eq!(
            client.join().unwrap(),
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
        );
    }

    #[test]
    fn project_info_response_has_a_versioned_stable_shape() {
        let body = serde_json::to_value(ProjectInfoResponse {
            app_control_protocol: 1,
            project: ProjectInfo {
                name: "Archive".into(),
                path: "/projects/archive.qrate".into(),
                source: Some("CSV".into()),
                created_at: Some("1234".into()),
                link_method: None,
                files_folder: None,
                row_count: 12,
                column_count: 4,
            },
        })
        .unwrap();
        assert_eq!(body["app_control_protocol"], 1);
        assert_eq!(body["project"]["name"], "Archive");
        assert_eq!(body["project"]["row_count"], 12);
        assert_eq!(body["project"]["column_count"], 4);
        assert!(body["project"]["link_method"].is_null());
    }
}
