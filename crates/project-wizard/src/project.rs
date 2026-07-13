//! Writes the on-disk result of "Create Project": a folder named after the
//! project containing a `project.qrate` SQLite file (see
//! `settings::project` for the v1 schema). Replaces the old `project.json`
//! placeholder manifest.

use std::path::Path;

pub use settings::project::ProjectColumn;

pub fn sanitize_folder_name(name: &str) -> String {
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

pub fn project_dir(save_dir: &str, name: &str) -> std::path::PathBuf {
    Path::new(save_dir).join(sanitize_folder_name(name))
}

pub fn name_taken(save_dir: &str, name: &str) -> bool {
    if save_dir.trim().is_empty() || name.trim().is_empty() {
        return false;
    }
    project_dir(save_dir, name).exists()
}

/// Loads the `.qrate` in `dir` and makes it the app-wide current project
/// (what the table and workspace read). Returns the project's display name.
pub fn open_project(dir: &Path, cx: &mut gpui::App) -> anyhow::Result<String> {
    let file = dir.join(settings::project::PROJECT_FILE_NAME);
    anyhow::ensure!(
        file.exists(),
        "no {} found in {}",
        settings::project::PROJECT_FILE_NAME,
        dir.display()
    );
    let data = settings::project::load_project_file(&file)?;
    let name = if data.name.is_empty() {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled Project".into())
    } else {
        data.name.clone()
    };
    cx.set_global(settings::project::CurrentProject {
        dir: dir.to_path_buf(),
        file,
        data,
    });
    Ok(name)
}

/// Creates the project folder and its `project.qrate` file; returns the
/// folder path (what the recents list and the launcher show).
#[allow(clippy::too_many_arguments)]
pub fn write_project_file(
    save_dir: &str,
    name: &str,
    source: &str,
    link_method: Option<&str>,
    columns: &[ProjectColumn],
    headers: &[String],
    rows: &[Vec<String>],
) -> anyhow::Result<String> {
    let dir = project_dir(save_dir, name);
    std::fs::create_dir_all(&dir)?;
    settings::project::create_project_file(
        &dir.join("project.qrate"),
        name,
        source,
        link_method,
        columns,
        headers,
        rows,
    )?;
    Ok(dir.to_string_lossy().to_string())
}
