//! "Explore an example project" on the first-run launcher: the repo's `sample/` collection, built
//! into a real `.qrate` the first time someone asks for it and reopened after that.
//!
//! The sample is compiled in rather than looked up next to the executable, so the button works the
//! same from an installer, a portable zip and `cargo run`. It is small — a four-row sheet, its
//! column config and four photos — which is what makes that affordable.
//!
//! The project is written under qrate's data folder, not beside the archivist's own work: it is
//! qrate's to recreate, and edits made while exploring it are kept until the file is deleted.

use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::data;
use crate::project;
use crate::steps::review::{column_settings, project_columns};

const PROJECT_NAME: &str = "Aderman Example";
const SHEET: &str = "aderman_collection.csv";
const CONFIG: &str = "column_config.csv";
const PHOTOS: &str = "photos";
/// The sheet has no column called Title, so the photographer's own caption stands in for one:
/// it is the field that reads as a name for each slide.
const TITLE_COLUMN: &str = "Photographer’s Note";
/// Declared `Filename` in the sample's column config — `1` resolves to `photos/1.jpg`.
const FILE_COLUMN: &str = "Digital ID";

const SAMPLE: [(&str, &[u8]); 6] = [
    (
        SHEET,
        include_bytes!("../../../sample/aderman_collection.csv"),
    ),
    (CONFIG, include_bytes!("../../../sample/column_config.csv")),
    (
        "photos/1.jpg",
        include_bytes!("../../../sample/photos/1.jpg"),
    ),
    (
        "photos/2.jpg",
        include_bytes!("../../../sample/photos/2.jpg"),
    ),
    (
        "photos/3.jpg",
        include_bytes!("../../../sample/photos/3.jpg"),
    ),
    (
        "photos/4.jpg",
        include_bytes!("../../../sample/photos/4.jpg"),
    ),
];

/// The launcher card's thumbnail: the first slide of the collection it opens.
pub(crate) const THUMBNAIL: &[u8] = include_bytes!("../../../sample/photos/1.jpg");

/// Where the example lives: its own folder in qrate's data directory.
fn example_dir() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("example"))
}

/// The example's `.qrate` file, created on first use. Later calls return the existing file
/// untouched, so whatever someone changed while exploring is still there next time.
pub(crate) fn ensure_example_project(cx: &gpui::App) -> anyhow::Result<PathBuf> {
    let dir = example_dir().context("couldn't find qrate's data folder")?;
    let file = project::project_file_path(&dir.to_string_lossy(), PROJECT_NAME);
    if file.exists() {
        return Ok(file);
    }
    write_sample(&dir)?;
    create_project(&dir, cx)?;
    Ok(file)
}

/// Unpacks the sample. Files already there are overwritten: a half-written folder from an earlier
/// failed attempt is the only way one exists without the project beside it.
fn write_sample(dir: &Path) -> anyhow::Result<()> {
    for (name, bytes) in SAMPLE {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("couldn't write {name}"))?;
    }
    Ok(())
}

/// The same `.qrate` the wizard would make from Spreadsheet + folder with the sample's column
/// config loaded, without the wizard: its columns, rows, and the column settings `__columns`
/// can't hold (authority, spell checking, severity).
fn create_project(dir: &Path, cx: &gpui::App) -> anyhow::Result<()> {
    let sheet = dir.join(SHEET);
    let preview = data::load_spreadsheet_preview(&sheet.to_string_lossy())
        .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
    let config = data::load_column_config(&dir.join(CONFIG).to_string_lossy(), &preview.headers)
        .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
    let columns = project_columns(&preview.headers, Some(&config), TITLE_COLUMN, FILE_COLUMN);
    let photos = dir.join(PHOTOS);
    let photos = photos.to_string_lossy();
    let file = project::write_project_file(
        &dir.to_string_lossy(),
        &project::ProjectSpec {
            name: PROJECT_NAME,
            source: "Spreadsheet + folder",
            link_method: Some("exact filename"),
            files_folder: Some(&photos),
            columns: &columns,
            headers: &preview.headers,
            rows: &preview.rows,
        },
    )?;
    for (key, value) in [
        (
            settings::onboarding::GUIDE_KEY,
            settings::onboarding::GUIDE_OPEN,
        ),
        (
            settings::onboarding::GUIDE_KIND_KEY,
            settings::onboarding::GuideKind::Media.as_str(),
        ),
    ] {
        settings::project::write_setting(Path::new(&file), key, value)?;
    }
    let settings = column_settings(&preview.headers, Some(&config), cx);
    if !settings.is_empty() {
        settings::project::write_setting(
            Path::new(&file),
            settings::columns::COLUMN_SETTINGS_KEY,
            &serde_json::to_string(&settings)?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The committed sample is what the launcher's example unpacks; a renamed column would leave
    /// it without a title or file role and fail only when somebody clicks the button.
    #[test]
    fn the_example_roles_are_columns_of_the_sample() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample");
        let headers =
            crate::data::load_spreadsheet_preview(root.join(super::SHEET).to_str().unwrap())
                .unwrap()
                .headers;
        for role in [super::TITLE_COLUMN, super::FILE_COLUMN] {
            assert!(
                headers.iter().any(|h| h == role),
                "{role} is not in the sample"
            );
        }
    }
}
