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

use settings::columns::ColumnType;
use std::path::PathBuf;

/// The disk walk stays in the table; the shared crate owns matching and tie-breaking.
pub struct PhotoIndex {
    inner: qrate_export::PhotoIndex,
    root: PathBuf,
}

impl PhotoIndex {
    pub fn build(folder: &str) -> Self {
        let root = PathBuf::from(folder);
        let paths = file_ingest::scan(&root, true)
            .map(|inventory| {
                inventory
                    .files()
                    .map(|entry| entry.relative_path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self {
            inner: qrate_export::PhotoIndex::from_paths(paths),
            root,
        }
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

/// Re-resolves the live grid's images against disk and hands them back to the delegate. The cached
/// paths were resolved from the text a filename cell held when the project opened, so an edit to
/// one leaves the Details panel showing the file the row used to name until this runs.
pub(crate) fn refresh(
    state: &mut gpui_component::table::TableState<crate::delegate::QrateTableDelegate>,
    cx: &gpui::App,
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
    let (headers, _, rows) = state.delegate().dataset_snapshot();
    let paths = resolve_row_images(&headers, &rows, &folder, &declared);
    state.delegate_mut().set_image_paths(paths);
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
