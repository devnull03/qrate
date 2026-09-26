//! Resolves each table row to an on-disk image path, for the Details panel. qrate never copies
//! source files into the project — only the files-folder path is persisted (see
//! `settings::project::FILES_FOLDER_KEY`) — so this index is rebuilt from disk every time a
//! project opens or the files folder changes.
//!
//! Real collections nest files inconsistently: some batches are flat
//! (`access/2021_05/2021_05_034.jpg`), others put one item's files a level deeper
//! (`access/2020_04/2020_04_001/2020_04_001_001.jpg`). A single non-recursive directory listing
//! can't resolve individual rows against that shape, so this walks the whole folder tree — mirroring
//! `index_access_files` in the CA migration scripts (`islandora_workbench/g/scripts/ca-migration/
//! prepare_ingest.py`), which solves the identical problem for the same source data.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Context, Global, SharedString};
use gpui_component::table::TableState;
use settings::columns::ColumnType;

use crate::delegate::QrateTableDelegate;

/// The disk walk stays in the table; the shared crate owns matching and tie-breaking.
pub struct PhotoIndex {
    inner: qrate_export::PhotoIndex,
    root: PathBuf,
    /// Every file the walk found, relative to `root`.
    pub(crate) files: Vec<PathBuf>,
}

impl PhotoIndex {
    pub fn build(folder: &str) -> Self {
        let root = PathBuf::from(folder);
        let files = file_ingest::scan(&root, true)
            .map(|inventory| {
                inventory
                    .files()
                    .map(|entry| entry.relative_path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self {
            inner: qrate_export::PhotoIndex::from_paths(files.clone()),
            root,
            files,
        }
    }

    /// This walk with `added` files and everything at or under `removed`, both relative to the
    /// root, without reading the folder again. `None` when that leaves the same set of files.
    pub(crate) fn changed(&self, added: &[PathBuf], removed: &[PathBuf]) -> Option<Self> {
        let gone: Vec<String> = removed.iter().map(|path| comparable(path)).collect();
        let kept: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|file| {
                let file = comparable(file);
                !gone.iter().any(|gone| within(&file, gone))
            })
            .cloned()
            .collect();
        let mut held: std::collections::HashSet<String> =
            kept.iter().map(|f| comparable(f)).collect();
        let new: Vec<PathBuf> = added
            .iter()
            .filter(|file| held.insert(comparable(file)))
            .cloned()
            .collect();
        if kept.len() == self.files.len() && new.is_empty() {
            return None;
        }
        let files: Vec<PathBuf> = kept.into_iter().chain(new).collect();
        Some(Self {
            inner: qrate_export::PhotoIndex::from_paths(files.clone()),
            root: self.root.clone(),
            files,
        })
    }

    pub(crate) fn resolve_cell(&self, cell: &str) -> Option<PathBuf> {
        self.inner.resolve_cell(cell).map(|path| {
            if path.is_absolute() {
                path
            } else {
                self.root.join(path)
            }
        })
    }

    fn resolve_row(
        &self,
        headers: &[String],
        declared: &[String],
        row: &[String],
    ) -> Option<PathBuf> {
        self.inner.resolve_row(headers, declared, row).map(|path| {
            if path.is_absolute() {
                path
            } else {
                self.root.join(path)
            }
        })
    }
}
/// The columns a project declares as holding a filename. Previews resolve against these before
/// guessing, so they judge the same cell the Problems panel reports on.
pub fn declared_file_columns(data: &settings::project::ProjectData) -> Vec<String> {
    data.columns
        .iter()
        .filter(|c| ColumnType::from_declared(&c.data_type) == ColumnType::Filename)
        .map(|c| c.name.clone())
        .collect()
}

/// Resolves every row to its image path (if any), for `QrateTableDelegate` to store alongside
/// the grid. `folder` is `settings::project::FILES_FOLDER_KEY`'s value — empty means no folder
/// was linked, so every row resolves to `None`.
pub fn resolve_row_images(
    headers: &[String],
    rows: &[Vec<String>],
    folder: &str,
    declared: &[String],
) -> Vec<Option<PathBuf>> {
    if folder.trim().is_empty() {
        return vec![None; rows.len()];
    }
    let index = PhotoIndex::build(folder);
    rows.iter()
        .map(|row| index.resolve_row(headers, declared, row))
        .collect()
}

/// The files folder's walk, shared by the grid's previews and `file_links`' checks. Rebuilt when
/// the folder changes, or after [`forget`] when the same folder may hold different files.
#[derive(Default)]
struct Walked {
    folder: String,
    index: Option<Arc<PhotoIndex>>,
}

impl Global for Walked {}

/// The walk of `folder`, if one is cached.
pub(crate) fn cached_index(folder: &str, cx: &App) -> Option<Arc<PhotoIndex>> {
    cx.try_global::<Walked>()
        .filter(|walked| walked.folder == folder)
        .and_then(|walked| walked.index.clone())
}

/// Drop the cached walk, so the next [`refresh`] reads the folder from disk — a relinked folder or
/// a reopened project can hold different files under the same path.
pub(crate) fn forget(cx: &mut App) {
    cx.set_global(Walked::default());
}

/// Keep `index` as the walk of `folder`.
pub(crate) fn remember(folder: String, index: Arc<PhotoIndex>, cx: &mut App) {
    cx.set_global(Walked {
        folder,
        index: Some(index),
    });
}

/// How two paths are compared: separators unified and case folded, as the import's duplicate
/// check compares them.
pub(crate) fn comparable(path: &std::path::Path) -> String {
    file_ingest::normalized_path(path).to_lowercase()
}

/// Whether the [`comparable`] path `path` is `folder` or lies under it. An empty `folder` is the
/// root, which holds everything.
pub(crate) fn within(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn strings(row: &[SharedString]) -> Vec<String> {
    row.iter().map(ToString::to_string).collect()
}

/// Re-resolve the grid's images against the files folder. `rows` limits it to the rows an edit
/// touched, which resolve on the spot against the cached walk; `None`, or no walk yet, resolves
/// every row off the UI thread and walks the folder first if it has to.
pub(crate) fn refresh(
    state: &mut TableState<QrateTableDelegate>,
    rows: Option<&[usize]>,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) {
    let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
        return;
    };
    let folder = project
        .data
        .values
        .get(settings::project::FILES_FOLDER_KEY)
        .map(|v| v.text().to_string())
        .unwrap_or_default();
    let declared = declared_file_columns(&project.data);
    let delegate = state.delegate_mut();
    if folder.trim().is_empty() {
        delegate.set_image_paths(vec![None; delegate.row_count()]);
        return;
    }
    let cached = cached_index(&folder, cx);
    if let (Some(rows), Some(index)) = (rows, &cached) {
        let images = rows
            .iter()
            .map(|&row| {
                let (headers, cells): (Vec<_>, Vec<_>) =
                    delegate.row_fields(row).into_iter().unzip();
                (
                    row,
                    index.resolve_row(&strings(&headers), &declared, &strings(&cells)),
                )
            })
            .collect();
        delegate.set_row_images(images);
        return;
    }
    let generation = delegate.values_generation();
    let (headers, grid) = delegate.grid();
    delegate.images_task = Some(cx.spawn(async move |this, cx| {
        let walk = folder.clone();
        let (index, paths) = cx
            .background_executor()
            .spawn(async move {
                let index = cached.unwrap_or_else(|| Arc::new(PhotoIndex::build(&walk)));
                let headers = strings(&headers);
                let paths: Vec<Option<PathBuf>> = grid
                    .iter()
                    .map(|row| index.resolve_row(&headers, &declared, &strings(row)))
                    .collect();
                (index, paths)
            })
            .await;
        let walked = cx.update(|cx| {
            let walked = cached_index(&folder, cx).is_none();
            if walked {
                remember(folder, index, cx);
            }
            walked
        });
        this.update(cx, |state, cx| {
            state.delegate_mut().images_task = None;
            if state.delegate().values_generation() != generation {
                refresh(state, None, cx);
                return;
            }
            state.delegate_mut().set_image_paths(paths);
            cx.emit(crate::TableChanged);
            cx.notify();
        })
        .ok();
        if walked {
            cx.update(crate::revalidate_now);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

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
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap(), &[]);
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
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap(), &[]);
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
    fn a_relative_path_disambiguates_duplicate_filenames() {
        let dir = tempdir("duplicate-basename");
        touch(&dir.join("photographs").join("001.jpg"));
        touch(&dir.join("documents").join("001.jpg"));

        let index = PhotoIndex::build(dir.to_str().unwrap());
        assert_eq!(
            index.resolve_cell("photographs/001.jpg"),
            Some(dir.join("photographs").join("001.jpg"))
        );
        assert_eq!(
            index.resolve_cell("documents/001.jpg"),
            Some(dir.join("documents").join("001.jpg"))
        );
    }

    #[test]
    fn an_item_id_resolves_to_the_first_of_its_parts() {
        // The row names the item; the folder holds its numbered parts. Whichever order the walk
        // reached them in, the panel shows part 001.
        let dir = tempdir("multi-part");
        touch(&dir.join("2020_04_001_002.jpg"));
        touch(&dir.join("2020_04_001_001.jpg"));

        let index = PhotoIndex::build(dir.to_str().unwrap());
        assert_eq!(
            index.resolve_cell("2020_04_001"),
            Some(dir.join("2020_04_001_001.jpg"))
        );
    }

    /// The sheet names the master, exported with its folder ahead of it; the linked folder holds
    /// the access derivative. The declared column is consulted first and still resolves.
    #[test]
    fn a_declared_column_naming_the_master_resolves_the_derivative() {
        let dir = tempdir("extension-drift");
        touch(&dir.join("2021_05_034.jpg"));

        let headers = vec!["Digital ID".into(), "Media".into()];
        let rows = vec![vec!["unrelated".into(), "masters/2021_05_034.TIF".into()]];
        let images = resolve_row_images(
            &headers,
            &rows,
            dir.to_str().unwrap(),
            &["Media".to_string()],
        );
        assert_eq!(images[0], Some(dir.join("2021_05_034.jpg")));
    }

    #[test]
    fn falls_back_to_any_cell_without_a_file_column() {
        let dir = tempdir("no-file-header");
        touch(&dir.join("photo1.png"));

        let headers = vec!["Digital ID".into(), "Notes".into()];
        let rows = vec![vec!["photo1".into(), "some notes".into()]];
        let images = resolve_row_images(&headers, &rows, dir.to_str().unwrap(), &[]);
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

        let empty = resolve_row_images(&[], &[vec![]], "", &[]);
        assert_eq!(empty, vec![None]);
        let missing = resolve_row_images(
            &["file".to_string()],
            &[vec!["2021_05_034.jpg".to_string()]],
            "/nonexistent/qrate-photos-folder",
            &[],
        );
        assert_eq!(missing, vec![None]);
    }
}
