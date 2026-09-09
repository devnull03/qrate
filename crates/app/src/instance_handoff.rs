//! Per-user handoff for browser links that launch a second qrate process.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const INSTANCE_NAME: &str = "io.github.devnull03.qrate.plugin-links";

pub fn start(link: Option<&str>, sender: async_channel::Sender<String>) -> bool {
    let Ok(instance) = single_instance::SingleInstance::new(INSTANCE_NAME) else {
        log::warn!("could not initialize plugin-link process handoff");
        return true;
    };
    if !instance.is_single() {
        if let Some(link) = link {
            log::info!("handing plugin install link to the running qrate instance");
            if let Err(error) = send(link) {
                log::error!("could not hand plugin link to the running qrate process: {error}");
            }
            return false;
        }
        return true;
    }

    let Some(inbox) = inbox() else {
        return true;
    };
    std::thread::Builder::new()
        .name("plugin-link-handoff".to_string())
        .spawn(move || watch(inbox, sender, instance))
        .map(|_| log::debug!("plugin install link handoff is listening"))
        .unwrap_or_else(|error| log::warn!("could not start plugin link handoff: {error}"));
    true
}

fn inbox() -> Option<PathBuf> {
    settings::data_dir().map(|data| data.join("plugin-link-inbox"))
}

fn send(link: &str) -> std::io::Result<()> {
    let inbox = inbox().ok_or_else(|| std::io::Error::other("application data is unavailable"))?;
    fs::create_dir_all(&inbox)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&inbox)?;
    temporary.write_all(link.as_bytes())?;
    temporary.as_file_mut().sync_all()?;
    let path = temporary.into_temp_path().keep()?;
    fs::rename(&path, path.with_extension("link"))
}

fn watch(
    inbox: PathBuf,
    sender: async_channel::Sender<String>,
    _instance: single_instance::SingleInstance,
) {
    if let Err(error) = fs::create_dir_all(&inbox) {
        log::warn!("could not create plugin-link handoff folder: {error}");
        return;
    }
    loop {
        if let Ok(entries) = fs::read_dir(&inbox) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "link") {
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
                    .filter(|link| link.len() <= 4096);
                let _ = fs::remove_file(path);
                if let Some(link) = link {
                    log::info!("received plugin install link from a second qrate process");
                    let _ = sender.send_blocking(link);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
