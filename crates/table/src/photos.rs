//! Resolves each table row to an on-disk image path, for the Details panel. qrate never copies
//! source files into the project — only the files-folder path is persisted (see
//! `settings::project::FILES_FOLDER_KEY`) — so this index is rebuilt from disk every time a
//! project opens or the files folder changes.
//!
//! Real collections nest files inconsistently: some batches are flat
//! (`access/2021_05/2021_05_034.jpg`), others put one item's files a level deeper
//! (`access/2020_04/2020_04_001/2020_04_001_001.jpg`). A single non-recursive directory listing
//! (as `project_wizard::data::match_folder` uses for its yes/no folder check) can't resolve
//! individual rows against that shape, so this walks the whole folder tree — mirroring
//! `index_access_files` in the CA migration scripts (`islandora_workbench/g/scripts/ca-migration/
//! prepare_ingest.py`), which solves the identical problem for the same source data.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gpui_component::IconName;

/// Whether `gpui`'s `img()` can decode this path. Anything else gets an icon instead of a
/// failed load + fallback, which also keeps the placeholder honest about *what* it stands for.
/// Matches gpui's `image_cache` decoders (`ImageFormat` + svg); extension-only, like the rest of
/// this module — sniffing magic bytes would mean reading every file off disk to pick an icon.
pub fn is_previewable_image(path: &Path) -> bool {
    matches!(
        extension(path).as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg")
    )
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

/// Placeholder icon for a file we can't render inline. `gpui_component`'s bundled icon set has
/// no media glyphs (no camera/film/music), so these are the nearest stand-ins available —
/// swap in custom SVGs via `Icon::path` if the set ever grows.
///
/// ponytail: four buckets, extension-keyed. Add a real mime crate only if the icon actually
/// needs to be right for files with no/wrong extension.
pub fn placeholder_icon(path: Option<&Path>) -> IconName {
    match path.and_then(extension).as_deref() {
        Some("pdf" | "epub" | "doc" | "docx" | "txt" | "md") => IconName::BookOpen,
        Some("mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "aiff") => IconName::Play,
        Some("mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v") => IconName::Frame,
        _ => IconName::File,
    }
}

/// Filenames a recursive walk should ignore — OS cruft, not collection data.
fn is_ignored(name: &str) -> bool {
    name.starts_with("._") || name.eq_ignore_ascii_case(".ds_store")
}

/// Every file under `folder`, found recursively, keyed by lowercased filename (with extension).
/// A depth-first stack walk rather than `walkdir` — the whole point of a recursive listing here
/// is a handful of nesting levels, not arbitrary directory trees, so std's `read_dir` is enough.
fn index_files(folder: &Path) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if is_ignored(name) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                index.entry(name.to_lowercase()).or_insert(path);
            }
        }
    }
    index
}

/// A recursive index of `folder`'s files, queryable by exact filename or filename stem
/// (case-insensitive) — the same two-tier match `project_wizard::data::match_folder` uses for
/// its folder-level check, just resolved per file instead of only counting matches.
pub struct PhotoIndex {
    by_name: HashMap<String, PathBuf>,
    by_stem: HashMap<String, PathBuf>,
}

impl PhotoIndex {
    /// Builds the index by walking `folder` recursively. An empty index (not an error) if
    /// `folder` doesn't exist or isn't readable — a missing/stale files folder degrades every
    /// row to "no image" rather than failing the whole project load.
    pub fn build(folder: &str) -> Self {
        let by_name = index_files(Path::new(folder));
        let by_stem = by_name
            .iter()
            .map(|(name, path)| {
                let stem = Path::new(name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name)
                    .to_string();
                (stem, path.clone())
            })
            .collect();
        Self { by_name, by_stem }
    }

    pub(crate) fn resolve_cell(&self, cell: &str) -> Option<&PathBuf> {
        let c = cell.trim().to_lowercase();
        if c.is_empty() {
            return None;
        }
        self.by_name.get(&c).or_else(|| self.by_stem.get(&c))
    }

    /// The image path for `row`'s cells, if any cell names a file this index found. Checks a
    /// "file"/"filename"-headed column first (the shape of the CA CSVs this was built against),
    /// then falls back to every cell — matching `match_folder`'s tolerance for spreadsheets
    /// without that exact header.
    fn resolve_row(&self, headers: &[String], row: &[String]) -> Option<PathBuf> {
        let file_col = headers.iter().position(|h| {
            let h = h.trim().to_lowercase();
            h == "file" || h == "filename"
        });
        if let Some(hit) = file_col
            .and_then(|ix| row.get(ix))
            .and_then(|c| self.resolve_cell(c))
        {
            return Some(hit.clone());
        }
        row.iter().find_map(|c| self.resolve_cell(c).cloned())
    }
}

/// Resolves every row to its image path (if any), for `QrateTableDelegate` to store alongside
/// the grid. `folder` is `settings::project::FILES_FOLDER_KEY`'s value — empty means no folder
/// was linked, so every row resolves to `None`.
pub fn resolve_row_images(
    headers: &[String],
    rows: &[Vec<String>],
    folder: &str,
) -> Vec<Option<PathBuf>> {
    if folder.trim().is_empty() {
        return vec![None; rows.len()];
    }
    let index = PhotoIndex::build(folder);
    rows.iter()
        .map(|row| index.resolve_row(headers, row))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("qrate-photos-test").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        write!(fs::File::create(path).unwrap(), "x").unwrap();
    }

    #[test]
    fn resolves_flat_folder() {
        // Mirrors access/2021_05: every image directly under the folder.
        let dir = tempdir("flat");
        touch(&dir.join("2021_05_034.jpg"));
        touch(&dir.join("2021_05_069.jpg"));

        let headers = vec!["id".into(), "file".into()];
        let rows = vec![
            vec!["2021_05_034".into(), "2021_05_034.jpg".into()],
            vec!["2021_05_069".into(), "2021_05_069.jpg".into()],
            vec!["2021_05_999".into(), "missing.jpg".into()],
        ];
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap());
        assert_eq!(images[0], Some(dir.join("2021_05_034.jpg")));
        assert_eq!(images[1], Some(dir.join("2021_05_069.jpg")));
        assert_eq!(images[2], None);
    }

    #[test]
    fn resolves_nested_folder() {
        // Mirrors access/2020_04: each item's files one level deeper, under its own id folder.
        let dir = tempdir("nested");
        touch(&dir.join("2020_04_001").join("2020_04_001_001.jpg"));
        touch(&dir.join("2020_04_001").join("2020_04_001_002.jpg"));

        let headers = vec!["id".into(), "file".into()];
        let rows = vec![
            vec!["2020_04_001_001".into(), "2020_04_001_001.jpg".into()],
            vec!["2020_04_001_002".into(), "2020_04_001_002.jpg".into()],
        ];
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap());
        assert_eq!(
            images[0],
            Some(dir.join("2020_04_001").join("2020_04_001_001.jpg"))
        );
        assert_eq!(
            images[1],
            Some(dir.join("2020_04_001").join("2020_04_001_002.jpg"))
        );
    }

    #[test]
    fn falls_back_to_any_cell_without_a_file_column() {
        let dir = tempdir("no-file-header");
        touch(&dir.join("photo1.png"));

        let headers = vec!["Digital ID".into(), "Notes".into()];
        let rows = vec![vec!["photo1".into(), "some notes".into()]];
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap());
        assert_eq!(images[0], Some(dir.join("photo1.png")));
    }

    #[test]
    fn ignores_os_cruft_and_missing_folder() {
        let dir = tempdir("cruft");
        touch(&dir.join("._2021_05_034.jpg"));
        touch(&dir.join(".DS_Store"));
        touch(&dir.join("2021_05_034.jpg"));

        let index = PhotoIndex::build(dir.to_str().unwrap());
        assert!(index.resolve_cell("2021_05_034.jpg").is_some());
        assert!(index.resolve_cell("._2021_05_034.jpg").is_none());
        assert!(index.resolve_cell(".ds_store").is_none());

        let empty = resolve_row_images(&[], &[vec![]], "");
        assert_eq!(empty, vec![None]);
        let missing = resolve_row_images(
            &["file".to_string()],
            &[vec!["2021_05_034.jpg".to_string()]],
            "/nonexistent/qrate-photos-folder",
        );
        assert_eq!(missing, vec![None]);
    }
}
