//! Filesystem discovery for project creation and live imports.
//!
//! This crate only inventories paths. It does not decide which qrate columns receive them or
//! mutate a project, so the wizard, table, and future folder watcher can share one traversal.

pub mod duplicates;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedComponent {
    pub title: String,
    pub absolute_path: PathBuf,
    pub source_path: PathBuf,
    pub kind: EntryKind,
    pub parent: Option<usize>,
    pub level_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub components: Vec<PlannedComponent>,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanOptions<'a> {
    pub recursive: bool,
    /// A dropped directory is itself an archival component. A folder merely selected as the
    /// project's file root can opt out and import only its contents.
    pub include_root: bool,
    pub folder_level_key: &'a str,
    pub file_level_key: &'a str,
}

impl Default for PlanOptions<'_> {
    fn default() -> Self {
        Self {
            recursive: true,
            include_root: true,
            folder_level_key: "series",
            file_level_key: "item",
        }
    }
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

/// Produces a side-effect-free component tree suitable for an import preview.
pub fn plan(root: &Path, options: &PlanOptions<'_>) -> Result<ImportPlan, Error> {
    Ok(plan_inventory(
        root,
        scan(root, options.recursive)?,
        options,
    ))
}

/// [`plan`] over an inventory already taken of `root`, so a caller that also matches the files
/// against something walks the folder once.
pub fn plan_inventory(root: &Path, inventory: Inventory, options: &PlanOptions<'_>) -> ImportPlan {
    let root_offset = usize::from(options.include_root);
    let mut components = Vec::with_capacity(inventory.entries.len() + root_offset);
    if options.include_root {
        components.push(PlannedComponent {
            title: display_name(root),
            absolute_path: root.to_path_buf(),
            source_path: PathBuf::new(),
            kind: EntryKind::Directory,
            parent: None,
            level_key: options.folder_level_key.to_string(),
        });
    }
    components.extend(inventory.entries.iter().map(|entry| {
        PlannedComponent {
            title: display_name(&entry.path),
            absolute_path: entry.path.clone(),
            source_path: entry.relative_path.clone(),
            kind: entry.kind,
            parent: entry
                .parent
                .map(|parent| parent + root_offset)
                .or(options.include_root.then_some(0)),
            level_key: match entry.kind {
                EntryKind::Directory => options.folder_level_key,
                EntryKind::File => options.file_level_key,
            }
            .to_string(),
        }
    }));
    ImportPlan {
        components,
        warnings: inventory.warnings,
    }
}

/// Plans one or more dropped files and directories as independent root components.
pub fn plan_paths(paths: &[PathBuf], options: &PlanOptions<'_>) -> Result<ImportPlan, Error> {
    if paths.is_empty() {
        return Err(Error::Empty);
    }
    let mut combined = ImportPlan {
        components: Vec::new(),
        warnings: Vec::new(),
    };
    for path in paths {
        if path.is_dir() {
            let mut next = plan(path, options)?;
            let offset = combined.components.len();
            for component in &mut next.components {
                component.parent = component.parent.map(|parent| parent + offset);
            }
            combined.components.extend(next.components);
            combined.warnings.extend(next.warnings);
        } else if path.is_file() {
            combined.components.push(PlannedComponent {
                title: display_name(path),
                absolute_path: path.clone(),
                source_path: path.clone(),
                kind: EntryKind::File,
                parent: None,
                level_key: options.file_level_key.to_string(),
            });
        } else {
            return Err(Error::NotDirectory);
        }
    }
    if combined.components.is_empty() {
        return Err(Error::Empty);
    }
    Ok(combined)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| normalized_path(path))
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
    // The type comes with the directory listing on every platform, so no entry is stat'ed again
    // except a symlink, whose target has to be asked about.
    let mut paths: Vec<(PathBuf, fs::FileType)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| Some((entry.path(), entry.file_type().ok()?)))
        .collect();
    paths.sort_by_cached_key(|(path, _)| {
        let relative = relative_string(root, path);
        (relative.to_lowercase(), relative)
    });

    for (path, file_type) in paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_ignored(name) {
            continue;
        }

        let (is_dir, is_file) = match file_type.is_symlink() {
            true => (path.is_dir(), path.is_file()),
            false => (file_type.is_dir(), file_type.is_file()),
        };
        if file_type.is_symlink() && is_dir {
            inventory
                .warnings
                .push(Warning::DirectorySymlinkSkipped(path));
            continue;
        }
        if is_dir {
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
        } else if is_file {
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

    use crate::{EntryKind, Error, PlanOptions, normalized_path, plan, plan_paths, scan};

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

    /// Folders sort among files by their whole relative path, case folded, and a case-only tie
    /// falls back to the exact bytes so two scans of one folder always agree.
    #[test]
    fn ordering_folds_case_across_folders_and_breaks_ties_exactly() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a_dir")).unwrap();
        fs::write(root.path().join("a_dir").join("x.jpg"), "x").unwrap();
        fs::write(root.path().join("b.jpg"), "b").unwrap();
        fs::write(root.path().join("A.jpg"), "a").unwrap();
        let paths = |root: &std::path::Path| -> Vec<String> {
            scan(root, true)
                .unwrap()
                .entries
                .iter()
                .map(|entry| normalized_path(&entry.relative_path))
                .collect()
        };
        assert_eq!(
            paths(root.path()),
            ["A.jpg", "a_dir", "a_dir/x.jpg", "b.jpg"]
        );

        // Only a case-sensitive file system can hold both spellings at once.
        fs::write(root.path().join("same.jpg"), "lower").unwrap();
        fs::write(root.path().join("Same.jpg"), "upper").unwrap();
        let listed = paths(root.path());
        if listed.iter().any(|path| path == "same.jpg") && listed.iter().any(|p| p == "Same.jpg") {
            let upper = listed.iter().position(|p| p == "Same.jpg").unwrap();
            let lower = listed.iter().position(|p| p == "same.jpg").unwrap();
            assert_eq!(lower, upper + 1, "exact bytes break the tie: {listed:?}");
        }
    }

    #[test]
    fn import_plan_includes_a_dropped_root_and_maps_levels() {
        let root = tempfile::tempdir().unwrap();
        let collection = root.path().join("Photographs");
        fs::create_dir_all(collection.join("Events")).unwrap();
        fs::write(collection.join("Events").join("001.jpg"), "photo").unwrap();

        let plan = plan(
            &collection,
            &PlanOptions {
                folder_level_key: "series",
                file_level_key: "item",
                ..Default::default()
            },
        )
        .unwrap();
        let components: Vec<_> = plan
            .components
            .iter()
            .map(|component| {
                (
                    component.title.as_str(),
                    component.kind,
                    component.parent,
                    component.level_key.as_str(),
                    normalized_path(&component.source_path),
                )
            })
            .collect();
        assert_eq!(
            components,
            [
                (
                    "Photographs",
                    EntryKind::Directory,
                    None,
                    "series",
                    "".into()
                ),
                (
                    "Events",
                    EntryKind::Directory,
                    Some(0),
                    "series",
                    "Events".into()
                ),
                (
                    "001.jpg",
                    EntryKind::File,
                    Some(1),
                    "item",
                    "Events/001.jpg".into(),
                ),
            ]
        );
    }

    #[test]
    fn selected_file_root_can_exclude_itself() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("one.jpg"), "photo").unwrap();
        let plan = plan(
            root.path(),
            &PlanOptions {
                include_root: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.components.len(), 1);
        assert_eq!(plan.components[0].parent, None);
        assert_eq!(plan.components[0].title, "one.jpg");
    }

    #[test]
    fn several_dropped_roots_keep_independent_parent_indexes() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("folder");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("inside.jpg"), "inside").unwrap();
        let loose = root.path().join("loose.jpg");
        fs::write(&loose, "loose").unwrap();

        let plan = plan_paths(&[folder, loose], &PlanOptions::default()).unwrap();
        assert_eq!(plan.components.len(), 3);
        assert_eq!(plan.components[0].parent, None);
        assert_eq!(plan.components[1].parent, Some(0));
        assert_eq!(plan.components[2].parent, None);
    }
}
