//! Per-user handoff for OS open requests that launch a second qrate process.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const INSTANCE_NAME: &str = "io.github.devnull03.qrate.open-requests";

pub fn start(request: Option<&str>, sender: async_channel::Sender<String>) -> bool {
    let Ok(instance) = single_instance::SingleInstance::new(INSTANCE_NAME) else {
        log::warn!("could not initialize OS open-request handoff");
        return true;
    };
    if !instance.is_single() {
        if let Some(request) = request {
            log::info!("handing an open request to the running qrate instance");
            return match send(request) {
                Ok(()) => false,
                Err(error) => {
                    log::error!(
                        "could not hand an open request to the running qrate process: {error}"
                    );
                    true
                }
            };
        }
        return true;
    }

    let Some(inbox) = inbox() else {
        return true;
    };
    std::thread::Builder::new()
        .name("os-open-handoff".to_string())
        .spawn(move || watch(inbox, sender, instance))
        .map(|_| log::debug!("OS open-request handoff is listening"))
        .unwrap_or_else(|error| log::warn!("could not start OS open-request handoff: {error}"));
    true
}

/// A tarball install registers its desktop entry and project MIME type when first launched.
#[cfg(target_os = "linux")]
pub fn register_linux_handlers() {
    let Ok(installation) = updater::detect_installation() else {
        return;
    };
    let Some(data) = dirs::data_dir() else {
        return;
    };
    let applications = data.join("applications");
    let mime = data.join("mime");
    let mime_package = mime.join("packages/io.github.devnull03.qrate.xml");
    let definition = include_str!("../../../assets/linux/qrate-mime.xml");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=qrate\nExec=\"{}\" %u\nIcon={}\nTerminal=false\n\
         Categories=Office;Database;\n\
         MimeType=application/x-qrate-project;inode/directory;x-scheme-handler/qrate;\n",
        installation
            .executable
            .to_string_lossy()
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\\\"")
            .replace('`', "\\\\`")
            .replace('$', "\\\\$")
            .replace('%', "%%"),
        installation.root.join("qrate.png").display(),
    );
    let path = applications.join("qrate.desktop");
    let receipt = data.join("qrate/desktop-registration");
    let registered_state = format!("{entry}{definition}");
    std::thread::spawn(move || {
        if fs::read_to_string(&path).is_ok_and(|current| current == entry)
            && fs::read_to_string(&mime_package).is_ok_and(|current| current == definition)
            && fs::read_to_string(&receipt).is_ok_and(|current| current == registered_state)
        {
            return;
        }
        let registered = (|| -> std::io::Result<()> {
            fs::create_dir_all(&applications)?;
            fs::create_dir_all(mime_package.parent().unwrap())?;
            fs::write(&path, &entry)?;
            fs::write(&mime_package, definition)?;
            let mut failure = None;
            for (program, arguments) in [
                ("update-mime-database", vec![mime.as_os_str()]),
                ("update-desktop-database", vec![applications.as_os_str()]),
                (
                    "xdg-mime",
                    vec![
                        "default".as_ref(),
                        "qrate.desktop".as_ref(),
                        "x-scheme-handler/qrate".as_ref(),
                    ],
                ),
            ] {
                let result = std::process::Command::new(program)
                    .args(arguments)
                    .output()
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(())
                        } else {
                            Err(std::io::Error::other(format!(
                                "{program} exited with {}: {}",
                                output.status,
                                String::from_utf8_lossy(&output.stderr).trim(),
                            )))
                        }
                    });
                if let Err(error) = result {
                    failure.get_or_insert(error);
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
            fs::create_dir_all(receipt.parent().unwrap())?;
            fs::write(receipt, registered_state)?;
            Ok(())
        })();
        match registered {
            Ok(()) => {
                log::info!("registered qrate projects, folders, and plugin links with the desktop")
            }
            Err(error) => {
                log::warn!("could not finish registering qrate's desktop handlers: {error}")
            }
        }
    });
}

fn inbox() -> Option<PathBuf> {
    settings::data_dir().map(|data| data.join("open-request-inbox"))
}

fn send(request: &str) -> std::io::Result<()> {
    let inbox = inbox().ok_or_else(|| std::io::Error::other("application data is unavailable"))?;
    fs::create_dir_all(&inbox)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&inbox)?;
    temporary.write_all(request.as_bytes())?;
    temporary.as_file_mut().sync_all()?;
    let path = temporary.into_temp_path().keep()?;
    fs::rename(&path, path.with_extension("request"))
}

fn watch(
    inbox: PathBuf,
    sender: async_channel::Sender<String>,
    _instance: single_instance::SingleInstance,
) {
    if let Err(error) = fs::create_dir_all(&inbox) {
        log::warn!("could not create OS open-request handoff folder: {error}");
        return;
    }
    loop {
        if let Ok(entries) = fs::read_dir(&inbox) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_none_or(|extension| extension != "request")
                {
                    continue;
                }
                let recent = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age <= Duration::from_secs(300));
                let link = recent
                    .then(|| fs::read_to_string(&path).ok())
                    .flatten()
                    .filter(|request| request.len() <= 32_768);
                let _ = fs::remove_file(path);
                if let Some(link) = link {
                    log::info!("received an open request from a second qrate process");
                    let _ = sender.send_blocking(link);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
