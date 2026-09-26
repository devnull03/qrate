//! Reports a row whose linked file isn't there.
//!
//! qrate never copies source files in — it stores the files folder's path and re-resolves against
//! it on open. So a folder that moved, a batch that was renamed, or one deleted scan all fail the
//! same silent way: the Details panel shows its no-image placeholder, which looks identical to a
//! row that never had a file. This turns that silence into a finding per row.
//!
//! Only a column *declared* [`ColumnType::Filename`] is checked. Row-level resolution
//! ([`PhotoIndex::resolve_row`]) guesses by scanning every cell, which is right for showing a
//! picture and wrong for reporting a problem — it cannot tell "this row has no file" from "this
//! value was supposed to be one".

use std::path::Path;

use diagnostics::{
    ColumnSnapshot, DATASET_MAIN, Diagnostics, Severity, Source, address as address_findings,
};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use settings::project::{CurrentProject, FILES_FOLDER_KEY};

use crate::delegate::QrateTableDelegate;
use crate::photos::PhotoIndex;

/// What the Problems panel shows in the source column, and the key this output is replaced by.
pub const SOURCE: &str = "files";

/// Below this many linked names, a folder holding few of them is not called moved.
const FEW_NAMES: usize = 5;

/// The files folder as a whole is wrong, rather than a file here and there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseProblem {
    Missing,
    MostlyMissing,
}

/// What the banner over the grid says about the files folder, and whether it was dismissed.
#[derive(Default)]
pub struct FilesBase {
    pub problem: Option<(BaseProblem, SharedString)>,
    pub dismissed: bool,
}

impl Global for FilesBase {}

/// The files folder's state for a folder that `exists`, holding `found` of the `total` names the
/// rows link to. Under a tenth found counts as moved, once there are enough names to judge by.
pub fn base_problem(exists: bool, found: usize, total: usize) -> Option<BaseProblem> {
    match exists {
        false => Some(BaseProblem::Missing),
        true if total >= FEW_NAMES && found * 10 < total => Some(BaseProblem::MostlyMissing),
        true => None,
    }
}

/// The distinct names the rows link to that the files folder has to answer for: every non-blank
/// cell except an absolute path to a file that exists, which needs no folder.
pub fn folder_names<'a>(values: impl IntoIterator<Item = &'a SharedString>) -> Vec<String> {
    let mut names: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !Path::new(value).is_file())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Check every `Filename` column against the files folder and publish what's missing.
///
/// Reads the walk `photos` keeps rather than walking the folder here, on the UI thread. With no
/// walk yet it asks the table for one and publishes nothing; the walk revalidates when it lands.
/// A folder that moved as a whole is one banner over the grid instead of a finding per row.
pub fn check(columns: &[ColumnSnapshot], cx: &mut App) {
    let folder = cx
        .try_global::<CurrentProject>()
        .and_then(|p| p.data.values.get(FILES_FOLDER_KEY).map(|v| v.text()))
        .unwrap_or_default()
        .to_string();

    let named: Vec<&ColumnSnapshot> = columns
        .iter()
        .filter(|c| ColumnType::from_declared(&c.data_type) == ColumnType::Filename)
        .collect();

    // No folder linked or nothing declared to hold a filename: publish nothing, which also clears
    // what a previous folder reported rather than leaving stale findings behind.
    if folder.trim().is_empty() || named.is_empty() {
        Diagnostics::set(&source(), DATASET_MAIN, Vec::new(), cx);
        let problem = (!folder.trim().is_empty())
            .then(|| base_problem(Path::new(&folder).is_dir(), 0, 0))
            .flatten();
        set_base(problem.map(|p| (p, folder.into())), cx);
        return;
    }

    let Some(index) = crate::photos::cached_index(&folder, cx) else {
        cx.defer(|cx| {
            if let Some(state) = cx
                .try_global::<crate::TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
            {
                state.update(cx, |state, cx| {
                    if state.delegate().images_task.is_none() {
                        crate::photos::refresh(state, None, cx);
                    }
                });
            }
        });
        return;
    };

    let names = folder_names(named.iter().flat_map(|column| column.values.iter()));
    let found = names
        .iter()
        .filter(|name| index.resolve_cell(name).is_some())
        .count();
    let problem = base_problem(Path::new(&folder).is_dir(), found, names.len());
    set_base(problem.map(|p| (p, folder.clone().into())), cx);

    let root = Path::new(&folder);
    let items: Vec<_> = named
        .iter()
        .flat_map(|column| {
            address_findings(
                SOURCE.into(),
                column,
                findings(&index, root, &column.values, problem.is_none())
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
        })
        .collect();
    Diagnostics::set(&source(), DATASET_MAIN, items, cx);
}

/// Publish the banner's verdict, only when it changed: a repeat would re-show a dismissed banner.
fn set_base(problem: Option<(BaseProblem, SharedString)>, cx: &mut App) {
    if cx.try_global::<FilesBase>().map(|base| &base.problem) != Some(&problem) {
        cx.set_global(FilesBase {
            problem,
            dismissed: false,
        });
    }
}

fn source() -> Source {
    Source::Validator(SOURCE.into())
}

/// Every row whose value names a file the folder doesn't hold, when `report_missing`, and every
/// row linked to a file outside `root`.
///
/// A blank cell claims nothing, so it is not a broken link — "this row has no file yet" is a
/// different problem from "this row points at a file that isn't there", and only the second one
/// is something qrate can be sure about.
fn findings(
    index: &PhotoIndex,
    root: &Path,
    values: &[SharedString],
    report_missing: bool,
) -> Vec<(usize, Severity, SharedString)> {
    values
        .iter()
        .enumerate()
        .filter(|(_, value)| !value.trim().is_empty())
        .filter_map(|(row, value)| match index.resolve_cell(value.trim()) {
            None if report_missing => Some((
                row,
                Severity::Error,
                format!("No file named “{}” in the files folder", value.trim()).into(),
            )),
            Some(path) if qrate_export::relative_to(root, &path).is_none() => Some((
                row,
                Severity::Warning,
                format!("“{}” is outside the project's files folder", path.display()).into(),
            )),
            _ => None,
        })
        .collect()
}

/// The declared Filename cells whose file the files folder's walk cannot find, as
/// `(row, col, value)`. Empty without a files folder or before the folder has been walked.
pub fn missing_files(delegate: &QrateTableDelegate, cx: &App) -> Vec<(usize, usize, SharedString)> {
    let Some(index) = files_index(cx) else {
        return Vec::new();
    };
    (0..delegate.column_count())
        .filter(|&col| delegate.column_type(col) == ColumnType::Filename)
        .flat_map(|col| {
            delegate
                .column_cells(col)
                .into_iter()
                .enumerate()
                .map(move |(row, value)| (row, col, value))
        })
        .filter(|(_, _, value)| {
            !value.trim().is_empty() && index.resolve_cell(value.trim()).is_none()
        })
        .collect()
}

/// A row's first declared Filename cell whose file cannot be found, as `(col, value)`.
pub fn missing_file(
    delegate: &QrateTableDelegate,
    row: usize,
    cx: &App,
) -> Option<(usize, SharedString)> {
    let index = files_index(cx)?;
    (0..delegate.column_count())
        .filter(|&col| delegate.column_type(col) == ColumnType::Filename)
        .filter_map(|col| Some((col, delegate.cell(row, col)?.clone())))
        .find(|(_, value)| !value.trim().is_empty() && index.resolve_cell(value.trim()).is_none())
}

fn files_index(cx: &App) -> Option<std::sync::Arc<PhotoIndex>> {
    let folder = cx
        .try_global::<CurrentProject>()?
        .data
        .values
        .get(FILES_FOLDER_KEY)?
        .text();
    if folder.trim().is_empty() {
        return None;
    }
    crate::photos::cached_index(&folder, cx)
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `diagnostics`' lib.rs test module.
    use crate::file_links::{BaseProblem, base_problem, findings, folder_names};
    use crate::photos::PhotoIndex;
    use diagnostics::Severity;
    use gpui::SharedString;
    use std::io::Write;

    fn missing(
        index: &PhotoIndex,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        findings(index, std::path::Path::new("/unused"), values, true)
            .into_iter()
            .filter(|(_, severity, _)| *severity == Severity::Error)
            .collect()
    }

    /// `case` names the folder, because tests run in parallel and a shared one means each test
    /// deletes the directory the other is writing into. Mirrors `photos::tests::tempdir`.
    fn folder_with(case: &str, names: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("qrate-file-links-test")
            .join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            write!(std::fs::File::create(dir.join(name)).unwrap(), "x").unwrap();
        }
        dir
    }

    #[test]
    fn only_a_value_that_names_an_absent_file_is_reported() {
        let dir = folder_with("absent", &["1.jpg", "2.jpg"]);
        let index = PhotoIndex::build(dir.to_str().unwrap());
        let values: Vec<SharedString> = ["1", "gone.jpg", "  ", "2.jpg"]
            .iter()
            .map(|v| SharedString::from(*v))
            .collect();

        let found = missing(&index, &values);
        assert_eq!(found.len(), 1, "only the one absent file");
        assert_eq!(found[0].0, 1, "and it is reported against its own row");
        assert!(found[0].2.contains("gone.jpg"), "named so it can be found");
    }

    /// The stem match the Details panel resolves by has to count here too, or every row whose
    /// cell holds an id rather than a filename would be reported as broken.
    #[test]
    fn a_bare_stem_resolves_the_way_the_details_panel_resolves_it() {
        let dir = folder_with("stem", &["2021_05_034.jpg"]);
        let index = PhotoIndex::build(dir.to_str().unwrap());
        let values = vec![SharedString::from("2021_05_034")];
        assert!(missing(&index, &values).is_empty());
    }

    /// Everything the panel can show a picture for has to pass here, or the row reads as broken
    /// while its own preview is on screen: an exported path, a master's extension over an access
    /// derivative, and an item id whose parts are only partly present.
    #[test]
    fn nothing_the_details_panel_can_resolve_is_reported_broken() {
        let dir = folder_with(
            "agreement",
            &["2021_05_034.jpg", "2020_04_001_002.jpg", "real.jpg"],
        );
        let index = PhotoIndex::build(dir.to_str().unwrap());
        let values: Vec<SharedString> = [
            "masters/2021_05_034.TIF",
            "2020_04_001",
            "real.jpg",
            "gone.jpg",
        ]
        .iter()
        .map(|v| SharedString::from(*v))
        .collect();

        let found = missing(&index, &values);
        assert_eq!(found.len(), 1, "only the file that truly isn't there");
        assert_eq!(found[0].0, 3);
    }

    #[test]
    fn a_moved_folder_is_called_once_there_are_enough_names_to_judge() {
        assert_eq!(base_problem(false, 0, 0), Some(BaseProblem::Missing));
        assert_eq!(base_problem(false, 9, 10), Some(BaseProblem::Missing));
        assert_eq!(base_problem(true, 0, 4), None, "too few names to judge");
        assert_eq!(base_problem(true, 0, 5), Some(BaseProblem::MostlyMissing));
        assert_eq!(base_problem(true, 9, 100), Some(BaseProblem::MostlyMissing));
        assert_eq!(base_problem(true, 10, 100), None, "a tenth found is enough");
        assert_eq!(base_problem(true, 5, 5), None);
    }

    /// An absolute path to a file that exists needs no folder, so it neither counts towards nor
    /// against one; blanks and repeats are one name or none.
    #[test]
    fn folder_names_skip_blanks_repeats_and_files_found_by_absolute_path() {
        let dir = folder_with("names", &["here.jpg"]);
        let absolute = dir.join("here.jpg").to_string_lossy().into_owned();
        let values: Vec<SharedString> =
            ["a.jpg", " a.jpg ", "", "b.jpg", &absolute, "C:/gone/x.jpg"]
                .iter()
                .map(|v| SharedString::from(v.to_string()))
                .collect();
        assert_eq!(folder_names(&values), ["C:/gone/x.jpg", "a.jpg", "b.jpg"]);
    }

    /// A file linked where it lies resolves, is not reported missing, and is reported as outside.
    #[test]
    fn an_absolute_path_outside_the_folder_resolves_with_a_warning() {
        let dir = folder_with("outside-root", &["1.jpg"]);
        let elsewhere = folder_with("outside-elsewhere", &["loose.jpg"]);
        let index = PhotoIndex::build(dir.to_str().unwrap());
        let loose = elsewhere.join("loose.jpg").to_string_lossy().into_owned();
        let inside = dir.join("1.jpg").to_string_lossy().into_owned();
        let values: Vec<SharedString> = [loose.as_str(), "1.jpg", inside.as_str()]
            .iter()
            .map(|v| SharedString::from(v.to_string()))
            .collect();

        let found = findings(&index, &dir, &values, true);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 0);
        assert_eq!(found[0].1, Severity::Warning);
        assert!(found[0].2.contains("outside the project's files folder"));
    }

    #[test]
    fn a_moved_base_reports_no_missing_rows() {
        let dir = folder_with("suppressed", &[]);
        let index = PhotoIndex::build(dir.to_str().unwrap());
        let values: Vec<SharedString> = ["1.jpg", "2.jpg"]
            .iter()
            .map(|v| SharedString::from(*v))
            .collect();
        assert_eq!(findings(&index, &dir, &values, true).len(), 2);
        assert!(findings(&index, &dir, &values, false).is_empty());
    }
}
