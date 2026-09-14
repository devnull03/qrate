//! Filesystem discovery for project creation and live imports.
//!
//! This crate only inventories paths. It does not decide which qrate columns receive them or
//! mutate a project, so the wizard, table, and future folder watcher can share one traversal.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The discovered path joined below the supplied root.
    pub path: PathBuf,
    /// Path below the supplied root. Directory entries never include the root itself.
    pub relative_path: PathBuf,
    pub kind: EntryKind,
    /// Index of the containing directory in [`Inventory::entries`]. `None` means the root.
    pub parent: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    UnreadableDirectory(PathBuf),
    DirectorySymlinkSkipped(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub warnings: Vec<Warning>,
}

impl Inventory {
    pub fn files(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
    }
}

#[derive(Debug)]
pub enum Error {
    NotDirectory,
    Permission(std::io::Error),
    Empty,
}

/// Inventory `root` in normalized lexical order.
///
/// A non-recursive scan returns files directly under the root. A recursive scan also returns
/// directory components and their descendants. Directory symlinks are not followed because an
/// import must terminate even when the filesystem contains a cycle.
pub fn scan(root: &Path, recursive: bool) -> Result<Inventory, Error> {
    if !root.is_dir() {
        return Err(Error::NotDirectory);
    }
    fs::read_dir(root).map_err(Error::Permission)?;

    let mut inventory = Inventory::default();
    visit(root, root, None, recursive, &mut inventory);
    if inventory.files().next().is_none() {
        return Err(Error::Empty);
    }
    Ok(inventory)
}

fn visit(
    root: &Path,
    directory: &Path,
    parent: Option<usize>,
    recursive: bool,
    inventory: &mut Inventory,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => {
            inventory
                .warnings
                .push(Warning::UnreadableDirectory(directory.to_path_buf()));
            return;
        }
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort_by(|left, right| {
        let (left, right) = (relative_string(root, left), relative_string(root, right));
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(&right))
    });

    for path in paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_ignored(name) {
            continue;
        }

        let Ok(file_type) = fs::symlink_metadata(&path).map(|meta| meta.file_type()) else {
            continue;
        };
        if file_type.is_symlink() && path.is_dir() {
            inventory
                .warnings
                .push(Warning::DirectorySymlinkSkipped(path));
            continue;
        }
        if path.is_dir() {
            if recursive {
                let index = inventory.entries.len();
                inventory.entries.push(Entry {
                    relative_path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                    path: path.clone(),
                    kind: EntryKind::Directory,
                    parent,
                });
                visit(root, &path, Some(index), true, inventory);
            }
        } else if path.is_file() {
            inventory.entries.push(Entry {
                relative_path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                path,
                kind: EntryKind::File,
                parent,
            });
        }
    }
}

fn relative_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn is_ignored(name: &str) -> bool {
    name.starts_with("._") || name.eq_ignore_ascii_case(".ds_store")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{EntryKind, Error, normalized_path, scan};

    #[test]
    fn recursive_inventory_preserves_directories_parents_and_duplicate_names() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::create_dir(root.path().join("b")).unwrap();
        fs::write(root.path().join("a").join("same.jpg"), "a").unwrap();
        fs::write(root.path().join("b").join("same.jpg"), "b").unwrap();

        let inventory = scan(root.path(), true).unwrap();
        let paths: Vec<_> = inventory
            .entries
            .iter()
            .map(|entry| {
                (
                    normalized_path(&entry.relative_path),
                    entry.kind,
                    entry.parent,
                )
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                ("a".into(), EntryKind::Directory, None),
                ("a/same.jpg".into(), EntryKind::File, Some(0)),
                ("b".into(), EntryKind::Directory, None),
                ("b/same.jpg".into(), EntryKind::File, Some(2)),
            ]
        );
    }

    #[test]
    fn non_recursive_inventory_returns_only_root_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("root.jpg"), "root").unwrap();
        fs::write(root.path().join("nested").join("inside.jpg"), "inside").unwrap();

        let inventory = scan(root.path(), false).unwrap();
        let paths: Vec<_> = inventory
            .files()
            .map(|entry| normalized_path(&entry.relative_path))
            .collect();
        assert_eq!(paths, ["root.jpg"]);
    }

    #[test]
    fn os_cruft_does_not_make_a_folder_nonempty() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".DS_Store"), "x").unwrap();
        fs::write(root.path().join("._photo.jpg"), "x").unwrap();
        assert!(matches!(scan(root.path(), true), Err(Error::Empty)));
    }

    #[test]
    fn ordering_is_case_insensitive_and_stable() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("z.jpg"), "z").unwrap();
        fs::write(root.path().join("A.jpg"), "a").unwrap();
        let paths: Vec<_> = scan(root.path(), true)
            .unwrap()
            .files()
            .map(|entry| normalized_path(&entry.relative_path))
            .collect();
        assert_eq!(paths, ["A.jpg", "z.jpg"]);
    }
}
