//! Recent-projects list shown on the launcher. Persisted through the same
//! `AppSettings` key-value store + `SettingsWriter` the rest of the app uses,
//! under one JSON-encoded key — there's no dedicated "projects" table yet.

use gpui::App;
use serde::{Deserialize, Serialize};
use settings::AppSettings;
use std::path::{Path, PathBuf};

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

/// Resolve a representative file without opening the project in the UI. The table uses this
/// same filename index; the launcher runs the scan off the UI thread and `preview::thumb` reads
/// its cached rendering when it paints the result.
pub fn preview_source(project_file: &Path) -> Option<PathBuf> {
    let data = settings::project::load_project_file(project_file).ok()?;
    let folder = data.values.get(settings::project::FILES_FOLDER_KEY)?.text();
    if folder.trim().is_empty() {
        return None;
    }
    let root = PathBuf::from(folder.as_ref());
    let paths = file_ingest::scan(&root, true).ok()?;
    let index = qrate_export::PhotoIndex::from_paths(
        paths.files().map(|entry| entry.relative_path.clone()),
    );
    let declared: Vec<_> = data
        .columns
        .iter()
        .filter(|column| {
            settings::columns::ColumnType::from_declared(&column.data_type)
                == settings::columns::ColumnType::Filename
        })
        .map(|column| column.name.clone())
        .collect();
    data.rows
        .iter()
        .find_map(|row| {
            let relative = index.resolve_row(&data.headers, &declared, row)?;
            let path = if relative.is_absolute() {
                relative
            } else {
                root.join(relative)
            };
            preview::can_preview(&path).then_some(path)
        })
        .or_else(|| {
            paths
                .files()
                .map(|entry| root.join(&entry.relative_path))
                .find(|path| preview::can_preview(path))
        })
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

/// Drops a project from the recent list. Only forgets the entry — the
/// `.qrate` file on disk is left untouched.
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
    let ago = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        ago(delta / 60, "minute")
    } else if delta < 86_400 {
        ago(delta / 3600, "hour")
    } else if delta < 86_400 * 7 {
        ago(delta / 86_400, "day")
    } else if delta < 86_400 * 30 {
        ago(delta / (86_400 * 7), "week")
    } else {
        ago(delta / (86_400 * 30), "month")
    }
}

#[cfg(test)]
mod tests {
    use super::preview_source;
    use settings::project::{ProjectColumn, ProjectSpec, create_project_file};

    #[test]
    fn recent_project_preview_resolves_nested_row_image() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        let nested = files.join("batch");
        std::fs::create_dir_all(&nested).unwrap();
        let image = nested.join("object.jpg");
        std::fs::write(&image, b"thumbnail source").unwrap();
        let project = temp.path().join("collection.qrate");
        let columns = [ProjectColumn {
            name: "Image".into(),
            data_type: "Filename".into(),
            notes: String::new(),
        }];
        let headers = ["Image".to_string()];
        let rows = [vec!["object.jpg".to_string()]];
        let folder = files.to_string_lossy();
        create_project_file(
            &project,
            &ProjectSpec {
                name: "Collection",
                files_folder: Some(&folder),
                columns: &columns,
                headers: &headers,
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(preview_source(&project), Some(image));
    }

    #[test]
    fn recent_project_preview_uses_linked_folder_without_image_rows() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("object.jpg");
        std::fs::write(&image, b"thumbnail source").unwrap();
        let project = temp.path().join("collection.qrate");
        let folder = files.to_string_lossy();
        create_project_file(
            &project,
            &ProjectSpec {
                name: "Collection",
                files_folder: Some(&folder),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(preview_source(&project), Some(image));
    }
}
