//! Filename resolution shared by the desktop table and export code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::filenames;

/// `path` below `root`, or `None` when it lies outside it. Windows paths compare without case,
/// as the file system does.
pub fn relative_to(root: &Path, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative.to_path_buf());
    }
    if !cfg!(windows) {
        return None;
    }
    let slashed = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let root = format!("{}/", slashed(root).trim_end_matches('/'));
    let path = slashed(path);
    path.to_ascii_lowercase()
        .starts_with(&root.to_ascii_lowercase())
        .then(|| PathBuf::from(&path[root.len()..]))
}

pub struct PhotoIndex {
    by_key: HashMap<String, PathBuf>,
}

impl PhotoIndex {
    pub fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut by_key = HashMap::new();
        for path in paths {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let relative = path.to_string_lossy().replace('\\', "/").to_lowercase();
            for key in std::iter::once(relative).chain(filenames::keys(name)) {
                by_key
                    .entry(key)
                    .and_modify(|kept: &mut PathBuf| {
                        if path < *kept {
                            *kept = path.clone();
                        }
                    })
                    .or_insert_with(|| path.clone());
            }
        }
        Self { by_key }
    }

    pub fn resolve_cell(&self, cell: &str) -> Option<PathBuf> {
        let direct = PathBuf::from(cell);
        if direct.is_absolute() && direct.is_file() {
            return Some(direct);
        }
        filenames::lookup_keys(cell)
            .iter()
            .find_map(|key| self.by_key.get(key).cloned())
    }

    pub fn resolve_row(
        &self,
        headers: &[String],
        declared: &[String],
        row: &[String],
    ) -> Option<PathBuf> {
        let file_col = headers
            .iter()
            .position(|h| declared.iter().any(|d| d == h))
            .or_else(|| {
                headers.iter().position(|h| {
                    let h = h.trim().to_lowercase();
                    h == "file" || h == "filename"
                })
            });
        if let Some(hit) = file_col
            .and_then(|ix| row.get(ix))
            .and_then(|c| self.resolve_cell(c))
        {
            return Some(hit);
        }
        row.iter().find_map(|c| self.resolve_cell(c))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{PhotoIndex, relative_to};

    #[test]
    fn a_path_is_relative_only_below_its_root() {
        let root = std::env::temp_dir().join("qrate-root");
        assert_eq!(
            relative_to(&root, &root.join("a").join("1.jpg")),
            Some(PathBuf::from("a").join("1.jpg"))
        );
        assert_eq!(
            relative_to(&root, &std::env::temp_dir().join("other.jpg")),
            None
        );
        assert_eq!(
            relative_to(
                &root,
                &std::env::temp_dir().join("qrate-root-sibling/x.jpg")
            ),
            None,
            "a folder sharing the root's name as a prefix is not inside it"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_compare_without_case() {
        assert_eq!(
            relative_to(
                std::path::Path::new(r"C:\Archive"),
                std::path::Path::new(r"c:\archive\Box\1.jpg")
            ),
            Some(PathBuf::from("Box/1.jpg"))
        );
    }

    #[test]
    fn nested_paths_and_component_order() {
        let index = PhotoIndex::from_paths([
            PathBuf::from("2020_04/2020_04_001/2020_04_001_001.jpg"),
            PathBuf::from("a-b/x.jpg"),
            PathBuf::from("a/b/x.jpg"),
        ]);
        assert_eq!(
            index.resolve_cell("2020_04_001"),
            Some(PathBuf::from("2020_04/2020_04_001/2020_04_001_001.jpg"))
        );
        assert_eq!(
            index.resolve_cell("x.jpg"),
            Some(PathBuf::from("a/b/x.jpg"))
        );
        assert_eq!(
            index.resolve_cell("a-b/x.jpg"),
            Some(PathBuf::from("a-b/x.jpg"))
        );
    }
}
