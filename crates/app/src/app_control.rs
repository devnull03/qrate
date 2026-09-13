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
    if request != "GET /status HTTP/1.1" || !authenticated {
        respond(&mut stream, "401 Unauthorized", "{}");
        return;
    }
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
    use super::respond;
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
}
