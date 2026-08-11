//! Diagnostics: the app-wide problem list — imported cell notes today, validators, spell-check,
//! and user marks later — plus the bottom-dock [`ProblemsPanel`] that lists them.
//!
//! A leaf crate on purpose: it never depends on `table`, so `table` can depend on *it* for
//! in-cell squiggles later. The cost of that inversion is [`DiagnosticHooks`], through which the
//! panel asks the app to reveal a cell.

pub mod fixes;
mod panel;
pub mod spelling;
mod validator;
pub use fixes::{Fix, FixProviders};
pub use panel::ProblemsPanel;
pub use validator::{
    AsyncValidators, ColumnInfo, ColumnSnapshot, ColumnValidator, Misspelling, SpellActions,
    Validators, address,
};

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{App, Global, Hsla, SharedString};
use gpui_component::ActiveTheme as _;

/// The one dataset a project can hold today. `__notes` keys by name so a second sheet is new
/// rows rather than a new table.
pub const DATASET_MAIN: &str = "dataset_main";

/// Where a problem points. `column` is the column *name*, which is `__columns`' PRIMARY KEY and
/// the table delegate's own column key too — one identity, addressed the same way everywhere.
///
/// Both coordinates are optional, and that is what distinguishes the kinds of note: row-only
/// marks a whole row, column-only a whole column, neither the dataset itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Location {
    pub dataset: SharedString,
    // ponytail: positional source index; becomes `dataset_main._row_id` once rows can be reordered.
    pub row: Option<usize>,
    pub column: Option<SharedString>,
}

/// How loud a problem is. Closed set — a hand-authored mark is just a [`Severity::Note`] from
/// [`Source::Note`], so marks and validator output share one list and one colour scale.
/// Ordered worst-first so sorting by it floats errors to the top.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    /// Stable `__notes.severity` spelling. Unknown text reads back as [`Severity::Note`] — a
    /// project written by a newer qrate stays readable, just flattened.
    pub fn key(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Note,
        }
    }
}

/// What emitted a problem. Doubles as the invalidation key for [`Diagnostics::set`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// A note attached to the data, whether typed here or carried in from the imported
    /// spreadsheet. Notes carry no provenance — one is worth no more than the other. Persists.
    Note,
    /// A named rule, validator, plugin, or language server — the string is what the panel shows.
    /// Never persisted: computed output is recomputed on open, and stored copies go stale.
    Validator(SharedString),
}

/// `__notes.source` for every persisted note, and the replace-by-source key [`Diagnostics::set`]
/// files them under.
pub const SOURCE_NOTE: &str = "note";

impl Source {
    /// What the panel shows in a diagnostic's source column.
    pub fn label(&self) -> SharedString {
        match self {
            Source::Note => SOURCE_NOTE.into(),
            Source::Validator(name) => name.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub location: Location,
    pub severity: Severity,
    pub source: Source,
    pub message: SharedString,
}

/// The palette `panels/log_viewer.rs` used to key off `[ERROR]`/`[WARN]` line prefixes, now
/// keyed off a real severity.
pub fn severity_color(severity: Severity, cx: &App) -> Hsla {
    let t = cx.theme();
    match severity {
        Severity::Error => t.danger,
        Severity::Warning => t.warning,
        Severity::Note => t.muted_foreground,
    }
}

/// Every open problem, from every source.
///
/// A plain-data global rather than an `Entity` behind a handle global (the `TableStateHandle`
/// shape): it is never rebuilt, so consumers need one `observe_global` and none of the
/// re-binding a dead `WeakEntity` would force on them. Mutating it wakes every observer once at
/// the end of the effect cycle, which is the batching a hand-rolled event queue would have been.
#[derive(Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    /// `items` indices grouped by row. Publishing happens on an edit; [`Self::at`] runs three
    /// times per rendered cell per frame, so the whole point is to turn that scan into a lookup
    /// over one row's handful of entries. Keyed by row alone because `Option<usize>` is `Copy` —
    /// a key carrying the column name would allocate a `SharedString` on every lookup and give
    /// the cost straight back.
    by_row: HashMap<Option<usize>, Vec<usize>>,
    /// Which project's stored notes are loaded. `CurrentProject` is mutated by every
    /// project-scoped setting write, so its observer fires far more often than the project
    /// actually changes.
    loaded: Option<PathBuf>,
}

impl Global for Diagnostics {}

impl Diagnostics {
    /// Publish `source`'s complete diagnostics for `dataset`, replacing whatever it published
    /// before (LSP `publishDiagnostics`). A re-run that finds nothing clears its own stale
    /// entries, so resolving a problem is just republishing without it.
    pub fn set(source: &Source, dataset: &str, items: Vec<Diagnostic>, cx: &mut App) {
        let this = cx.default_global::<Self>();
        this.items
            .retain(|d| &d.source != source || d.location.dataset != dataset);
        this.items.extend(items);
        this.reindex();
    }

    fn reindex(&mut self) {
        self.by_row.clear();
        for (ix, diagnostic) in self.items.iter().enumerate() {
            self.by_row
                .entry(diagnostic.location.row)
                .or_default()
                .push(ix);
        }
    }

    /// Everything currently open, unsorted — the panel sorts and filters for display.
    pub fn all(cx: &App) -> &[Diagnostic] {
        cx.try_global::<Self>().map_or(&[], |d| d.items.as_slice())
    }

    /// `(errors, warnings)` for the status bar, which shows them apart — one error and one warning
    /// are not the same news, and a single total hides which of the two you have. Notes are
    /// excluded entirely: a project full of imported notes should not light up an alert.
    pub fn counts(cx: &App) -> (usize, usize) {
        Self::all(cx)
            .iter()
            .fold((0, 0), |(errors, warnings), d| match d.severity {
                Severity::Error => (errors + 1, warnings),
                Severity::Warning => (errors, warnings + 1),
                Severity::Note => (errors, warnings),
            })
    }

    /// Everything pointing at exactly this location. A cell passes both coordinates, the row-index
    /// gutter passes row-only, a column header column-only — so a cell does *not* inherit its
    /// row's or column's diagnostics, and each is marked where it was attached.
    ///
    /// Narrowed by [`Self::by_row`] first, so what remains to compare is one row's entries rather
    /// than the whole sheet's.
    pub fn at<'a>(
        dataset: &'a str,
        row: Option<usize>,
        column: Option<&'a str>,
        cx: &'a App,
    ) -> impl Iterator<Item = &'a Diagnostic> {
        cx.try_global::<Self>()
            .into_iter()
            .flat_map(move |this| {
                this.by_row
                    .get(&row)
                    .map_or([].as_slice(), Vec::as_slice)
                    .iter()
                    .map(move |&ix| &this.items[ix])
            })
            .filter(move |d| {
                d.location.dataset == dataset && d.location.column.as_deref() == column
            })
    }

    /// Severity of the loudest diagnostic here, which is the colour the corner marker takes.
    pub fn worst_at(
        dataset: &str,
        row: Option<usize>,
        column: Option<&str>,
        cx: &App,
    ) -> Option<Severity> {
        Self::at(dataset, row, column, cx).map(|d| d.severity).min()
    }

    /// The note filed here, if any — what the `Notes ▸` menu offers to edit rather than add.
    pub fn note_at(
        dataset: &str,
        row: Option<usize>,
        column: Option<&str>,
        cx: &App,
    ) -> Option<SharedString> {
        Self::at(dataset, row, column, cx)
            .find(|d| d.source == Source::Note)
            .map(|d| d.message.clone())
    }

    /// Attach a note here, replacing any note already at this location; an empty `message` deletes
    /// it. Writes straight through to `__notes` — this is a deliberate keystroke, not the hot path
    /// the debounced setting writer exists for.
    pub fn set_note(location: Location, message: SharedString, cx: &mut App) {
        let this = cx.default_global::<Self>();
        this.items
            .retain(|d| d.source != Source::Note || d.location != location);
        if !message.trim().is_empty() {
            this.items.push(Diagnostic {
                location,
                severity: Severity::Note,
                source: Source::Note,
                message,
            });
        }
        this.reindex();
        this.persist();
    }

    /// Follow a row insert: every note at or below `at` is now one row further down. Computed
    /// findings are left alone — the revalidate that follows a structural edit republishes them.
    pub fn rows_inserted(dataset: &str, at: usize, count: usize, cx: &mut App) {
        Self::remap_rows(dataset, cx, |row| {
            Some(if row >= at { row + count } else { row })
        });
    }

    /// Follow a row delete: notes on a deleted row go with it, and everything below it moves up by
    /// however many of the deleted rows were above it.
    pub fn rows_removed(dataset: &str, removed: &[usize], cx: &mut App) {
        Self::remap_rows(dataset, cx, |row| {
            (!removed.contains(&row)).then(|| row - removed.iter().filter(|&&r| r < row).count())
        });
    }

    /// Follow a column rename, which re-keys every note addressed to the old name.
    pub fn column_renamed(dataset: &str, before: &str, after: &SharedString, cx: &mut App) {
        let this = cx.default_global::<Self>();
        for item in &mut this.items {
            if item.location.dataset == dataset && item.location.column.as_deref() == Some(before) {
                item.location.column = Some(after.clone());
            }
        }
        this.persist();
    }

    /// Move every note in `dataset` to the row `f` gives back, dropping the ones it answers `None`
    /// for. The shared half of [`rows_inserted`](Self::rows_inserted) and its delete counterpart.
    fn remap_rows(dataset: &str, cx: &mut App, f: impl Fn(usize) -> Option<usize>) {
        let this = cx.default_global::<Self>();
        this.items.retain_mut(|item| {
            if item.location.dataset != dataset {
                return true;
            }
            let Some(row) = item.location.row else {
                return true;
            };
            match f(row) {
                Some(moved) => {
                    item.location.row = Some(moved);
                    true
                }
                None => false,
            }
        });
        this.reindex();
        this.persist();
    }

    /// Write every authored note back to `__notes`, replacing what was there. Computed findings
    /// are never persisted — they are recomputed on open and a stored copy would go stale.
    fn persist(&self) {
        // No open project (tests, early startup) leaves the change in memory only.
        let Some(file) = self.loaded.as_ref() else {
            return;
        };
        let notes: Vec<_> = self
            .items
            .iter()
            .filter(|d| d.source == Source::Note)
            .map(|d| settings::project::StoredNote {
                dataset: d.location.dataset.to_string(),
                row: d.location.row,
                column: d.location.column.as_ref().map(|c| c.to_string()),
                severity: d.severity.key().into(),
                message: d.message.to_string(),
            })
            .collect();
        if let Err(err) = settings::project::write_notes(file, SOURCE_NOTE, &notes) {
            log::error!("failed to save notes: {err}");
        }
    }
}

/// Load the open project's stored notes, and reload on project switch. Called from `main` rather
/// than the panel so the status-bar count is live even with the Problems panel closed.
pub fn init(cx: &mut App) {
    load_project_notes(cx);
    cx.observe_global::<settings::project::CurrentProject>(load_project_notes)
        .detach();
}

fn load_project_notes(cx: &mut App) {
    let Some(file) = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|p| p.file.clone())
    else {
        return;
    };
    if cx
        .try_global::<Diagnostics>()
        .is_some_and(|d| d.loaded.as_ref() == Some(&file))
    {
        return;
    }

    let stored = settings::project::read_notes(&file).unwrap_or_default();
    cx.default_global::<Diagnostics>().loaded = Some(file);

    // Republished under `Source::Note`, exactly as if a note had just been attached, so the
    // store reaches the same state whichever way a note arrived.
    let items = stored
        .iter()
        .map(|n| Diagnostic {
            location: Location {
                dataset: n.dataset.clone().into(),
                row: n.row,
                column: n.column.clone().map(SharedString::from),
            },
            severity: Severity::from_key(&n.severity),
            source: Source::Note,
            message: n.message.clone().into(),
        })
        .collect();
    Diagnostics::set(&Source::Note, DATASET_MAIN, items, cx);
}

/// Set once at startup (see `crates/app/src/main.rs`) so the Problems panel can jump the table to
/// a cell without this crate depending on `table` — that edge has to stay free for
/// `table -> diagnostics` (in-cell squiggles) later.
#[derive(Clone, Copy)]
pub struct DiagnosticHooks {
    pub reveal: fn(&Location, &mut App),
    /// The text currently at a location, so the Problems panel can build a fix menu from the cell
    /// itself rather than from the diagnostic's message, which is prose for a human.
    pub text_at: fn(&Location, &App) -> Option<SharedString>,
    /// Write text back to a location and revalidate. The other half of [`Self::text_at`], and the
    /// reason a panel row can offer the same corrections a cell does.
    pub set_text: fn(&Location, SharedString, &mut App),
}

impl Global for DiagnosticHooks {}

#[cfg(test)]
mod tests {
    // Never `use super::*` here: this module's parent has `use gpui::*` in scope transitively,
    // and the chained glob makes gpui's `test` macro shadow the `#[test]` its own expansion
    // emits, recursing until rustc's stack overflows.
    use crate::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source, init};
    use gpui::{SharedString, TestAppContext};
    use settings::project::{CurrentProject, ProjectData, StoredNote};

    /// The row index is a second copy of the truth in `items`, so every mutation has to rebuild
    /// it. This is the test that fails if a future one forgets.
    #[gpui::test]
    fn the_row_index_survives_every_mutation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let at_00 =
                |cx: &gpui::App| Diagnostics::at(DATASET_MAIN, Some(0), Some("Title"), cx).count();

            Diagnostics::set(
                &Source::Validator("v".into()),
                DATASET_MAIN,
                vec![diag(
                    Severity::Error,
                    Source::Validator("v".into()),
                    DATASET_MAIN,
                    "bad",
                )],
                cx,
            );
            assert_eq!(at_00(cx), 1);

            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                column: Some("Title".into()),
            };
            Diagnostics::set_note(location.clone(), "hand written".into(), cx);
            assert_eq!(at_00(cx), 2, "the note joins the validator's finding");

            // An empty message deletes the note, which shifts every later index in `items`.
            Diagnostics::set_note(location, "".into(), cx);
            assert_eq!(at_00(cx), 1);

            // Republishing nothing is how a validator clears itself.
            Diagnostics::set(&Source::Validator("v".into()), DATASET_MAIN, Vec::new(), cx);
            assert_eq!(at_00(cx), 0);
        });
    }

    fn diag(severity: Severity, source: Source, dataset: &str, msg: &str) -> Diagnostic {
        Diagnostic {
            location: Location {
                dataset: SharedString::from(dataset.to_string()),
                row: Some(0),
                column: Some("Title".into()),
            },
            severity,
            source,
            message: msg.into(),
        }
    }

    #[gpui::test]
    fn stored_notes_load_when_a_project_opens(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("qrate-diagnostics-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("load.qrate");
        let _ = std::fs::remove_file(&file);

        settings::project::create_project_file(&file, "T", "CSV", None, None, &[], &[], &[])
            .unwrap();
        settings::project::write_notes(
            &file,
            "note",
            &[
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(2),
                    column: Some("Title".into()),
                    severity: "warning".into(),
                    message: "check this".into(),
                },
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(4),
                    column: None,
                    severity: "note".into(),
                    message: "whole row".into(),
                },
            ],
        )
        .unwrap();

        cx.update(|cx| {
            cx.set_global(CurrentProject {
                file: file.clone(),
                data: ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: Vec::new(),
                    rows: Vec::new(),
                    values: Default::default(),
                },
            });
            init(cx);

            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 2);
            assert_eq!(
                Diagnostics::counts(cx),
                (0, 1),
                "the note doesn't count, the warning does"
            );

            // Severity and the optional coordinates survive the disk round-trip, and the source
            // comes back as `Import` so a re-import replaces exactly these.
            let wide = all.iter().find(|d| d.message == "whole row").unwrap();
            assert_eq!(wide.severity, Severity::Note);
            assert_eq!((wide.location.row, &wide.location.column), (Some(4), &None));
            assert!(all.iter().all(|d| d.source == Source::Note));

            // Re-running is idempotent: the `loaded` guard stops the same project's notes being
            // read again on every project-scoped setting write.
            init(cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);
        });
    }

    #[gpui::test]
    fn notes_upsert_delete_and_reach_the_disk(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("qrate-diagnostics-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("author.qrate");
        let _ = std::fs::remove_file(&file);
        settings::project::create_project_file(&file, "T", "CSV", None, None, &[], &[], &[])
            .unwrap();

        let cell = Location {
            dataset: DATASET_MAIN.into(),
            row: Some(3),
            column: Some("Title".into()),
        };
        let whole_row = Location {
            dataset: DATASET_MAIN.into(),
            row: Some(7),
            column: None,
        };

        cx.update(|cx| {
            cx.set_global(CurrentProject {
                file: file.clone(),
                data: ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: Vec::new(),
                    rows: Vec::new(),
                    values: Default::default(),
                },
            });
            init(cx);

            Diagnostics::set_note(cell.clone(), "first".into(), cx);
            Diagnostics::set_note(whole_row.clone(), "row wide".into(), cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);

            // Same location again replaces rather than stacking.
            Diagnostics::set_note(cell.clone(), "second".into(), cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(3), Some("Title"), cx),
                Some("second".into())
            );

            // A validator sharing the cell is a separate entry, and outranks the note's colour.
            let v = Source::Validator("spell".into());
            Diagnostics::set(
                &v,
                DATASET_MAIN,
                vec![Diagnostic {
                    location: cell.clone(),
                    severity: Severity::Error,
                    source: v.clone(),
                    message: "typo".into(),
                }],
                cx,
            );
            assert_eq!(
                Diagnostics::worst_at(DATASET_MAIN, Some(3), Some("Title"), cx),
                Some(Severity::Error)
            );

            // An empty message deletes the note and leaves the validator's entry standing.
            Diagnostics::set_note(cell.clone(), "   ".into(), cx);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(3), Some("Title"), cx),
                None
            );
            assert_eq!(
                Diagnostics::worst_at(DATASET_MAIN, Some(3), Some("Title"), cx),
                Some(Severity::Error)
            );
        });

        // Only the surviving note is on disk — validator output never is.
        let stored = settings::project::read_notes(&file).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].message, "row wide");
        assert_eq!((stored[0].row, &stored[0].column), (Some(7), &None));
    }

    /// A note is addressed by row position, so a row inserted above it has to carry it down — and
    /// a deleted row takes its own notes with it rather than leaving them on its successor.
    #[gpui::test]
    fn notes_follow_the_rows_they_are_attached_to(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let note = |row: usize| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(row),
                column: Some("Title".into()),
            };
            Diagnostics::set_note(note(1), "one".into(), cx);
            Diagnostics::set_note(note(5), "five".into(), cx);

            Diagnostics::rows_inserted(DATASET_MAIN, 3, 1, cx);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(1), Some("Title"), cx),
                Some("one".into()),
                "above the insert, unmoved"
            );
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(6), Some("Title"), cx),
                Some("five".into())
            );

            // Two rows go, one of them the one holding "one".
            Diagnostics::rows_removed(DATASET_MAIN, &[1, 2], cx);
            assert_eq!(Diagnostics::all(cx).len(), 1, "\"one\" went with its row");
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(4), Some("Title"), cx),
                Some("five".into()),
                "shifted up by both deletions"
            );

            // A rename re-keys the column half of the address.
            Diagnostics::column_renamed(DATASET_MAIN, "Title", &"Caption".into(), cx);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(4), Some("Caption"), cx),
                Some("five".into())
            );
        });
    }

    #[gpui::test]
    fn publishing_replaces_only_that_sources_diagnostics(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let import = |n: usize| {
                (0..n)
                    .map(|i| diag(Severity::Note, Source::Note, DATASET_MAIN, &format!("n{i}")))
                    .collect::<Vec<_>>()
            };

            Diagnostics::set(&Source::Note, DATASET_MAIN, import(2), cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);

            // Republishing replaces rather than appends — 1, not 3.
            Diagnostics::set(&Source::Note, DATASET_MAIN, import(1), cx);
            assert_eq!(Diagnostics::all(cx).len(), 1);

            // A different source is additive.
            let spell = Source::Validator("spell".into());
            Diagnostics::set(
                &spell,
                DATASET_MAIN,
                vec![diag(Severity::Error, spell.clone(), DATASET_MAIN, "typo")],
                cx,
            );
            assert_eq!(Diagnostics::all(cx).len(), 2);

            // Publishing nothing clears that source and leaves the other alone.
            Diagnostics::set(&Source::Note, DATASET_MAIN, vec![], cx);
            assert_eq!(Diagnostics::all(cx).len(), 1);
            assert_eq!(Diagnostics::all(cx)[0].source, spell);

            // ...and only within its own dataset.
            Diagnostics::set(
                &spell,
                "sheet2",
                vec![diag(Severity::Error, spell.clone(), "sheet2", "other")],
                cx,
            );
            assert_eq!(Diagnostics::all(cx).len(), 2);
        });
    }

    #[gpui::test]
    fn the_status_counts_split_by_severity_and_ignore_notes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            Diagnostics::set(
                &Source::Note,
                DATASET_MAIN,
                vec![
                    diag(Severity::Error, Source::Note, DATASET_MAIN, "e"),
                    diag(Severity::Warning, Source::Note, DATASET_MAIN, "w"),
                    diag(Severity::Note, Source::Note, DATASET_MAIN, "n"),
                ],
                cx,
            );
            assert_eq!(Diagnostics::all(cx).len(), 3);
            assert_eq!(Diagnostics::counts(cx), (1, 1));
        });
    }
}
