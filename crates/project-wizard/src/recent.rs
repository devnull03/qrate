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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<CachedPreview>,
}

/// A recent project's launcher thumbnail, remembered with the folder it was found in so the
/// launcher doesn't walk that folder (often an external drive) every time it opens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedPreview {
    pub folder: String,
    /// `None` when the folder held nothing previewable.
    pub image: Option<PathBuf>,
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

/// The launcher thumbnail for a recent project. `cached` is reused while the project still links
/// the same folder and its file is still there; a folder that isn't reachable (an unplugged drive)
/// is never scanned, and keeps its cache for when it's back. `None` when there's nothing to show
/// or to remember.
pub fn preview(project_file: &Path, cached: Option<CachedPreview>) -> Option<CachedPreview> {
    let folder = settings::project::read_setting(project_file, settings::project::FILES_FOLDER_KEY)
        .ok()??;
    if folder.trim().is_empty() {
        return None;
    }
    let reachable = Path::new(&folder).is_dir();
    if let Some(cached) = cached.filter(|cached| cached.folder == folder)
        && cached
            .image
            .as_ref()
            .is_none_or(|image| !reachable || image.exists())
    {
        return Some(cached);
    }
    if !reachable {
        return None;
    }
    let image = scan_preview(project_file, Path::new(&folder));
    Some(CachedPreview { folder, image })
}

/// Walks `root` for a representative file: the first row's linked file, else any previewable one.
/// Uses the same filename index the table does.
fn scan_preview(project_file: &Path, root: &Path) -> Option<PathBuf> {
    let data = settings::project::load_project_file(project_file).ok()?;
    let paths = file_ingest::scan(root, true).ok()?;
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

/// Remembers freshly resolved thumbnails, keyed by project path. Entries dropped from the list
/// while the scan ran are skipped.
pub fn store_previews(previews: Vec<(String, CachedPreview)>, cx: &mut App) {
    let mut recents = list(cx);
    let mut changed = false;
    for (path, preview) in previews {
        if let Some(project) = recents.iter_mut().find(|p| p.path == path)
            && project.preview.as_ref() != Some(&preview)
        {
            project.preview = Some(preview);
            changed = true;
        }
    }
    if changed && let Ok(json) = serde_json::to_string(&recents) {
        AppSettings::set_text(RECENTS_KEY, json.into(), cx);
    }
}

pub fn record_opened(name: String, path: String, cx: &mut App) {
    let mut recents = list(cx);
    let preview = recents
        .iter()
        .position(|p| p.path == path)
        .and_then(|ix| recents.remove(ix).preview);
    recents.insert(
        0,
        RecentProject {
            name,
            path,
            opened_at_unix: now_unix(),
            preview,
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
    use std::path::{Path, PathBuf};

    use super::{CachedPreview, preview};
    use settings::project::{ProjectColumn, ProjectSpec, create_project_file};

    /// A project linking `folder`, with one Filename column when `rows` is non-empty.
    fn project_linking(dir: &Path, folder: &Path, rows: &[&str]) -> PathBuf {
        let project = dir.join("collection.qrate");
        let columns = [ProjectColumn {
            name: "Image".into(),
            data_type: "Filename".into(),
            notes: String::new(),
        }];
        let headers = ["Image".to_string()];
        let rows: Vec<Vec<String>> = rows.iter().map(|row| vec![row.to_string()]).collect();
        let folder = folder.to_string_lossy();
        create_project_file(
            &project,
            &ProjectSpec {
                name: "Collection",
                files_folder: Some(&folder),
                columns: &columns[..usize::from(!rows.is_empty())],
                headers: &headers[..usize::from(!rows.is_empty())],
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();
        project
    }

    fn image_of(project: &Path, cached: Option<CachedPreview>) -> Option<PathBuf> {
        preview(project, cached).and_then(|preview| preview.image)
    }

    #[test]
    fn recent_project_preview_resolves_nested_row_image() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        let nested = files.join("batch");
        std::fs::create_dir_all(&nested).unwrap();
        let image = nested.join("object.jpg");
        std::fs::write(&image, b"thumbnail source").unwrap();
        let project = project_linking(temp.path(), &files, &["object.jpg"]);

        assert_eq!(image_of(&project, None), Some(image));
    }

    #[test]
    fn recent_project_preview_uses_linked_folder_without_image_rows() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("object.jpg");
        std::fs::write(&image, b"thumbnail source").unwrap();
        let project = project_linking(temp.path(), &files, &[]);

        assert_eq!(image_of(&project, None), Some(image));
    }

    /// The point of the cache: a folder on an external drive is walked once, not per launcher.
    #[test]
    fn a_cached_preview_is_reused_without_rescanning() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        std::fs::create_dir(&files).unwrap();
        std::fs::write(files.join("scanned.jpg"), b"x").unwrap();
        let remembered = files.join("remembered.jpg");
        std::fs::write(&remembered, b"x").unwrap();
        let project = project_linking(temp.path(), &files, &[]);
        let cached = CachedPreview {
            folder: files.to_string_lossy().into_owned(),
            image: Some(remembered.clone()),
        };

        assert_eq!(image_of(&project, Some(cached)), Some(remembered));
    }

    #[test]
    fn an_unplugged_drive_keeps_its_cache_and_is_not_scanned() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("unplugged");
        let project = project_linking(temp.path(), &files, &[]);
        let cached = CachedPreview {
            folder: files.to_string_lossy().into_owned(),
            image: Some(files.join("object.jpg")),
        };

        assert_eq!(preview(&project, Some(cached.clone())), Some(cached));
        assert_eq!(preview(&project, None), None, "nothing to remember yet");
    }

    #[test]
    fn a_cache_for_another_folder_or_a_deleted_file_is_rescanned() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("photos");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("object.jpg");
        std::fs::write(&image, b"x").unwrap();
        let project = project_linking(temp.path(), &files, &[]);
        let deleted = CachedPreview {
            folder: files.to_string_lossy().into_owned(),
            image: Some(files.join("gone.jpg")),
        };
        let relinked = CachedPreview {
            folder: "elsewhere".into(),
            image: None,
        };

        assert_eq!(image_of(&project, Some(deleted)), Some(image.clone()));
        assert_eq!(image_of(&project, Some(relinked)), Some(image));
    }
}
