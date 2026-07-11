//! Writes the on-disk result of "Create Project". qrate has no project data
//! model yet (see `crates/app/src/actions.rs`'s `NewProject` stub comment),
//! so this writes a small `project.json` manifest into a folder named after
//! the project — enough for the wizard to produce a real, inspectable result
//! until a proper project format lands.

use std::path::Path;

use serde::Serialize;

#[derive(Serialize)]
pub struct ManifestColumn {
    pub name: String,
    pub data_type: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct ProjectManifest {
    pub name: String,
    pub source: String,
    pub files_matched: Option<(usize, usize)>,
    pub link_method: Option<String>,
    pub columns: Vec<ManifestColumn>,
}

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

pub fn write_project_file(
    save_dir: &str,
    name: &str,
    manifest: &ProjectManifest,
) -> anyhow::Result<String> {
    let dir = project_dir(save_dir, name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("project.json");
    let json = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&path, json)?;
    Ok(dir.to_string_lossy().to_string())
}
