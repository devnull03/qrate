//! Writes the on-disk result of "Create Project": a single `<name>.qrate`
//! SQLite file in the chosen save folder (see `settings::project` for the v1
//! schema). Replaces the old folder-per-project layout — the file *is* the
//! project.

use std::path::Path;

pub use settings::project::{ProjectColumn, StoredNote, write_notes};

pub fn sanitize_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Untitled Project".into()
    } else {
        trimmed.into()
    }
}

pub fn project_file_path(save_dir: &str, name: &str) -> std::path::PathBuf {
    Path::new(save_dir).join(format!("{}.qrate", sanitize_file_stem(name)))
}

pub fn name_taken(save_dir: &str, name: &str) -> bool {
    if save_dir.trim().is_empty() || name.trim().is_empty() {
        return false;
    }
    project_file_path(save_dir, name).exists()
}

/// Loads the `.qrate` at `file` and makes it the app-wide current project
/// (what the table and workspace read). Returns the project's display name.
pub fn open_project(file: &Path, cx: &mut gpui::App) -> anyhow::Result<String> {
    anyhow::ensure!(file.exists(), "no project file at {}", file.display());
    let data = settings::project::load_project_file(file)?;
    let project = settings::project::CurrentProject {
        file: file.to_path_buf(),
        data,
    };
    let name = project.display_name();
    cx.set_global(project);
    Ok(name)
}

/// Creates the `<name>.qrate` file in `save_dir`; returns the file path
/// (what the recents list and the launcher show). `files_folder` is only
/// persisted (as a path) — qrate never copies the files themselves; row
/// images are re-resolved against this folder every time the project opens.
#[allow(clippy::too_many_arguments)]
pub fn write_project_file(
    save_dir: &str,
    name: &str,
    source: &str,
    link_method: Option<&str>,
    files_folder: Option<&str>,
    columns: &[ProjectColumn],
    headers: &[String],
    rows: &[Vec<String>],
) -> anyhow::Result<String> {
    std::fs::create_dir_all(save_dir)?;
    let file = project_file_path(save_dir, name);
    settings::project::create_project_file(
        &file,
        name,
        source,
        link_method,
        files_folder,
        columns,
        headers,
        rows,
    )?;
    Ok(file.to_string_lossy().to_string())
}
