//! Recent-projects list shown on the launcher. Persisted through the same
//! `AppSettings` key-value store + `SettingsWriter` the rest of the app uses,
//! under one JSON-encoded key — there's no dedicated "projects" table yet.

use gpui::App;
use serde::{Deserialize, Serialize};
use settings::AppSettings;

const RECENTS_KEY: &str = "project_wizard.recent_projects";
const MAX_RECENTS: usize = 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
    pub opened_at_unix: i64,
}

pub fn list(cx: &App) -> Vec<RecentProject> {
    let raw = AppSettings::get(cx)
        .values
        .get(RECENTS_KEY)
        .map(|v| v.text())
        .unwrap_or_default();
    if raw.is_empty() {
        return Vec::new();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn record_opened(name: String, path: String, cx: &mut App) {
    let mut recents = list(cx);
    recents.retain(|p| p.path != path);
    recents.insert(
        0,
        RecentProject {
            name,
            path,
            opened_at_unix: now_unix(),
        },
    );
    recents.truncate(MAX_RECENTS);
    let Ok(json) = serde_json::to_string(&recents) else {
        return;
    };
    AppSettings::set_text(RECENTS_KEY, json.into(), cx);
}

/// Drops a project from the recent list. Only forgets the entry — the project
/// folder on disk is left untouched.
pub fn remove(path: &str, cx: &mut App) {
    let mut recents = list(cx);
    let before = recents.len();
    recents.retain(|p| p.path != path);
    if recents.len() == before {
        return;
    }
    let Ok(json) = serde_json::to_string(&recents) else {
        return;
    };
    AppSettings::set_text(RECENTS_KEY, json.into(), cx);
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn relative_time(opened_at_unix: i64) -> String {
    let now = now_unix();
    let delta = (now - opened_at_unix).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        format!("{} minutes ago", delta / 60)
    } else if delta < 86_400 {
        format!("{} hours ago", delta / 3600)
    } else if delta < 86_400 * 7 {
        format!("{} days ago", delta / 86_400)
    } else if delta < 86_400 * 30 {
        format!("{} weeks ago", delta / (86_400 * 7))
    } else {
        format!("{} months ago", delta / (86_400 * 30))
    }
}
