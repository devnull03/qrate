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

use diagnostics::{
    ColumnSnapshot, DATASET_MAIN, Diagnostics, Severity, Source, address as address_findings,
};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use settings::project::{CurrentProject, FILES_FOLDER_KEY};

use crate::photos::PhotoIndex;

/// What the Problems panel shows in the source column, and the key this output is replaced by.
pub const SOURCE: &str = "files";

/// The walk, kept between runs and rebuilt only when the folder changes.
///
/// Validation runs on every commit, and walking a collection's folder tree per keystroke is not
/// something to do on the UI thread. The folder is a path in the project file, so it only changes
/// when someone re-links it.
// ponytail: not watched — a file added or deleted outside qrate is noticed on the next re-link
// or project open. Watch the folder if that gap starts confusing people.
#[derive(Default)]
struct Walked {
    folder: String,
    index: Option<PhotoIndex>,
}

impl Global for Walked {}

/// Check every `Filename` column against the files folder and publish what's missing.
///
/// Registered as a deferred producer because of the walk, not because it answers late: it
/// publishes before returning, so a resolved link clears on the same commit that fixed it.
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
        return;
    }

    let cached = cx.default_global::<Walked>();
    if cached.index.is_none() || cached.folder != folder {
        cached.index = Some(PhotoIndex::build(&folder));
        cached.folder = folder;
    }
    let Some(index) = cx.global::<Walked>().index.as_ref() else {
        return;
    };

    let items: Vec<_> = named
        .iter()
        .flat_map(|column| address_findings(SOURCE.into(), column, missing(index, &column.values)))
        .collect();
    Diagnostics::set(&source(), DATASET_MAIN, items, cx);
}

fn source() -> Source {
    Source::Validator(SOURCE.into())
}

/// Every row whose value names a file the folder doesn't hold.
///
/// A blank cell claims nothing, so it is not a broken link — "this row has no file yet" is a
/// different problem from "this row points at a file that isn't there", and only the second one
/// is something qrate can be sure about.
fn missing(index: &PhotoIndex, values: &[SharedString]) -> Vec<(usize, Severity, SharedString)> {
    values
        .iter()
        .enumerate()
        .filter(|(_, value)| !value.trim().is_empty())
        .filter(|(_, value)| index.resolve_cell(value).is_none())
        .map(|(row, value)| {
            (
                row,
                Severity::Error,
                format!("No file named “{}” in the files folder", value.trim()).into(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `diagnostics`' lib.rs test module.
    use crate::file_links::missing;
    use crate::photos::PhotoIndex;
    use gpui::SharedString;
    use std::io::Write;

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
}
