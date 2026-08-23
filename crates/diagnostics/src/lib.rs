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
    /// Live zero-based source position, used for rendering and navigation.
    pub row: Option<usize>,
    /// Stable `dataset_main._row_id`, used only when an authored note reaches disk.
    pub row_id: Option<settings::project::RowId>,
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
    /// spreadsheet. Persists, and carries whatever provenance it arrived with — see [`Filed`].
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
    /// Who filed this and when, for authored notes. Always `None` on a computed finding — a
    /// validator's output is recomputed on open, so it has no history to carry.
    pub filed: Option<Filed>,
}

/// A note's provenance. Free text rather than a parsed date: a catalogue inherits notes from
/// whatever kept them before, and refusing to store "March 1998" because it is not ISO-8601 loses
/// the note to keep the field tidy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Filed {
    pub date: Option<SharedString>,
    pub author: Option<SharedString>,
}

impl Filed {
    /// `1998-03-04 · MA`, or whichever half exists. `None` when neither does, so a caller can drop
    /// the line rather than render an empty one.
    pub fn label(&self) -> Option<SharedString> {
        match (self.date.as_ref(), self.author.as_ref()) {
            (Some(date), Some(author)) => Some(format!("{date} · {author}").into()),
            (Some(only), None) | (None, Some(only)) => Some(only.clone()),
            (None, None) => None,
        }
    }
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

    /// The note filed here, if any — what the `Notes ▸` menu offers to edit rather than add. The
    /// *first* of them: editing is how a single note is corrected, while a second observation
    /// about the same item is added rather than overwriting the first.
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

    /// Every note filed here, oldest first — what the Notes panel lists. An item accumulates
    /// observations over decades of cataloguing, and each is somebody's separate act.
    pub fn notes_at<'a>(
        dataset: &'a str,
        row: Option<usize>,
        column: Option<&'a str>,
        cx: &'a App,
    ) -> impl Iterator<Item = &'a Diagnostic> {
        Self::at(dataset, row, column, cx).filter(|d| d.source == Source::Note)
    }

    /// How many notes are filed here.
    pub fn note_count(dataset: &str, row: Option<usize>, column: Option<&str>, cx: &App) -> usize {
        Self::notes_at(dataset, row, column, cx).count()
    }

    /// Every note anywhere on one row — the ones filed on its cells as well as on the row itself,
    /// oldest first. [`Self::at`] deliberately does *not* let a row inherit its cells' diagnostics,
    /// because the grid marks each where it was attached; but a view with no cells to mark — the
    /// gallery's tile, the Details panel — is describing the *item*, and to it "dated 1962 is a
    /// guess" filed on the Date column is a note about the photograph.
    pub fn notes_in_row<'a>(
        dataset: &'a str,
        row: usize,
        cx: &'a App,
    ) -> impl Iterator<Item = &'a Diagnostic> {
        cx.try_global::<Self>()
            .into_iter()
            .flat_map(move |this| {
                this.by_row
                    .get(&Some(row))
                    .map_or([].as_slice(), Vec::as_slice)
                    .iter()
                    .map(move |&ix| &this.items[ix])
            })
            .filter(move |d| d.location.dataset == dataset && d.source == Source::Note)
    }

    /// File another note here without disturbing the ones already at this location. `set_note`'s
    /// counterpart: that one corrects, this one adds.
    pub fn add_note(location: Location, message: SharedString, cx: &mut App) {
        if message.trim().is_empty() {
            return;
        }
        let filed = Self::filed_now(cx);
        let this = cx.default_global::<Self>();
        this.items.push(Diagnostic {
            location,
            severity: Severity::Note,
            source: Source::Note,
            message,
            filed,
        });
        this.reindex();
        this.persist();
    }

    /// Stamp for a note being filed right now: today's date from the project file's own clock, and
    /// whoever the archivist has told the app they are. Either half may be missing.
    fn filed_now(cx: &App) -> Option<Filed> {
        let date = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|p| settings::project::today(&p.file))
            .map(SharedString::from);
        // Guarded: a note can be filed before the settings globals exist (early startup, tests),
        // and an unsigned note is a far better outcome than a panic mid-keystroke.
        let author = match cx.has_global::<settings::AppSettings>() {
            false => None,
            true => match settings::effective_text(settings::NOTE_AUTHOR_KEY, cx) {
                author if author.trim().is_empty() => None,
                author => Some(author),
            },
        };
        (date.is_some() || author.is_some()).then_some(Filed { date, author })
    }

    /// Attach a note here, replacing any note already at this location; an empty `message` deletes
    /// it. Writes straight through to `__notes` — this is a deliberate keystroke, not the hot path
    /// the debounced setting writer exists for.
    pub fn set_note(location: Location, message: SharedString, cx: &mut App) {
        // Keeps the original filing stamp: correcting a transcription is not re-observing the
        // item, and re-dating it to today would erase when the observation was actually made.
        let filed = Self::notes_at(
            &location.dataset,
            location.row,
            location.column.as_deref(),
            cx,
        )
        .next()
        .and_then(|d| d.filed.clone())
        .or_else(|| Self::filed_now(cx));
        let this = cx.default_global::<Self>();
        this.items
            .retain(|d| d.source != Source::Note || d.location != location);
        if !message.trim().is_empty() {
            this.items.push(Diagnostic {
                location,
                severity: Severity::Note,
                source: Source::Note,
                message,
                filed,
            });
        }
        this.reindex();
        this.persist();
    }

    /// Rebind authored notes to the current source positions after a structural edit or its undo.
    /// Their stable ids decide ownership; positions are only the live UI address.
    pub fn align_note_rows(dataset: &str, row_ids: &[settings::project::RowId], cx: &mut App) {
        let positions: HashMap<_, _> = row_ids
            .iter()
            .copied()
            .enumerate()
            .map(|(source, id)| (id, source))
            .collect();
        let this = cx.default_global::<Self>();
        let mut changed = false;
        this.items.retain_mut(|item| {
            if item.location.dataset != dataset || item.source != Source::Note {
                return true;
            }
            let Some(row_id) = item.location.row_id else {
                return true;
            };
            let Some(source) = positions.get(&row_id).copied() else {
                changed = true;
                return false;
            };
            changed |= item.location.row != Some(source);
            item.location.row = Some(source);
            true
        });
        if changed {
            this.reindex();
            this.persist();
        }
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
                row_id: d.location.row_id,
                column: d.location.column.as_ref().map(|c| c.to_string()),
                severity: d.severity.key().into(),
                message: d.message.to_string(),
                created_at: d
                    .filed
                    .as_ref()
                    .and_then(|f| f.date.as_ref().map(SharedString::to_string)),
                author: d
                    .filed
                    .as_ref()
                    .and_then(|f| f.author.as_ref().map(SharedString::to_string)),
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
                row_id: n.row_id,
                column: n.column.clone().map(SharedString::from),
            },
            severity: Severity::from_key(&n.severity),
            source: Source::Note,
            message: n.message.clone().into(),
            filed: match (&n.created_at, &n.author) {
                (None, None) => None,
                (date, author) => Some(Filed {
                    date: date.clone().map(SharedString::from),
                    author: author.clone().map(SharedString::from),
                }),
            },
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
    use crate::{DATASET_MAIN, Diagnostic, Diagnostics, Filed, Location, Severity, Source, init};
    use gpui::{App, SharedString, TestAppContext};
    use settings::project::{CurrentProject, ProjectData, ProjectSpec, StoredNote};

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
                row_id: None,
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

    /// The gallery has no cells to mark, so a note filed on one field still has to reach it —
    /// the bug this fixes was a tile and a Details panel showing "no notes" for a row somebody had
    /// just annotated in the grid.
    #[gpui::test]
    fn a_note_on_a_cell_counts_as_a_note_on_its_row(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let at = |column: Option<&str>| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(3),
                row_id: None,
                column: column.map(Into::into),
            };
            Diagnostics::add_note(at(Some("Date taken")), "1962 is a guess".into(), cx);
            Diagnostics::add_note(at(None), "whole print is faded".into(), cx);

            assert_eq!(
                Diagnostics::note_count(DATASET_MAIN, Some(3), None, cx),
                1,
                "the grid still marks each note only where it was attached"
            );
            assert_eq!(
                Diagnostics::notes_in_row(DATASET_MAIN, 3, cx).count(),
                2,
                "a view of the whole item sees both"
            );
            assert_eq!(
                Diagnostics::notes_in_row("other", 3, cx).count(),
                0,
                "and only within its own dataset"
            );
        });
    }

    fn diag(severity: Severity, source: Source, dataset: &str, msg: &str) -> Diagnostic {
        Diagnostic {
            location: Location {
                dataset: SharedString::from(dataset.to_string()),
                row: Some(0),
                row_id: None,
                column: Some("Title".into()),
            },
            severity,
            source,
            message: msg.into(),
            filed: None,
        }
    }

    /// An item accumulates observations over decades. `add_note` files another beside the ones
    /// already there; `set_note` corrects a single one and — the part worth pinning — keeps its
    /// original filing stamp, because fixing a typo is not re-observing the object in 2026.
    #[gpui::test]
    fn notes_accumulate_and_a_correction_keeps_its_original_date(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let cell = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(1),
                row_id: None,
                column: None,
            };
            let filed = |cx: &App| {
                Diagnostics::notes_at(DATASET_MAIN, Some(1), None, cx)
                    .map(|d| (d.message.to_string(), d.filed.clone()))
                    .collect::<Vec<_>>()
            };

            Diagnostics::add_note(cell.clone(), "verso inscription".into(), cx);
            Diagnostics::add_note(cell.clone(), "same backdrop as row 12".into(), cx);
            assert_eq!(
                Diagnostics::note_count(DATASET_MAIN, Some(1), None, cx),
                2,
                "a second observation joins the first rather than replacing it"
            );

            // Backdate the first, as a note loaded from an older catalogue would be.
            let original = Some(Filed {
                date: Some("1998-03-04".into()),
                author: Some("MA".into()),
            });
            cx.default_global::<Diagnostics>().items[0].filed = original.clone();

            // set_note collapses to one and keeps that stamp.
            Diagnostics::set_note(cell, "verso inscription, in pencil".into(), cx);
            let notes = filed(cx);
            assert_eq!(notes.len(), 1, "a correction replaces rather than adds");
            assert_eq!(notes[0].0, "verso inscription, in pencil");
            assert_eq!(
                notes[0].1, original,
                "the filing date is not moved to today"
            );
        });
    }

    /// Both halves are optional, and a note with neither must not render an empty byline.
    #[test]
    fn a_filing_stamp_reads_as_whichever_halves_exist() {
        let filed = |date: Option<&str>, author: Option<&str>| Filed {
            date: date.map(SharedString::from),
            author: author.map(SharedString::from),
        };
        assert_eq!(
            filed(Some("1998-03-04"), Some("MA")).label().as_deref(),
            Some("1998-03-04 · MA")
        );
        assert_eq!(
            filed(Some("1998-03-04"), None).label().as_deref(),
            Some("1998-03-04")
        );
        assert_eq!(filed(None, Some("rk")).label().as_deref(), Some("rk"));
        assert_eq!(filed(None, None).label(), None);
    }

    #[gpui::test]
    fn stored_notes_load_when_a_project_opens(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("qrate-diagnostics-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("load.qrate");
        let _ = std::fs::remove_file(&file);

        settings::project::create_project_file(
            &file,
            &ProjectSpec {
                name: "T",
                source: "CSV",
                ..Default::default()
            },
        )
        .unwrap();
        settings::project::write_notes(
            &file,
            "note",
            &[
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(2),
                    row_id: Some(2),
                    column: Some("Title".into()),
                    severity: "warning".into(),
                    message: "check this".into(),
                    created_at: None,
                    author: None,
                },
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(4),
                    row_id: Some(4),
                    column: None,
                    severity: "note".into(),
                    message: "whole row".into(),
                    created_at: None,
                    author: None,
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
                    row_ids: Vec::new(),
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
        settings::project::create_project_file(
            &file,
            &ProjectSpec {
                name: "T",
                source: "CSV",
                ..Default::default()
            },
        )
        .unwrap();

        let cell = Location {
            dataset: DATASET_MAIN.into(),
            row: Some(3),
            row_id: Some(3),
            column: Some("Title".into()),
        };
        let whole_row = Location {
            dataset: DATASET_MAIN.into(),
            row: Some(7),
            row_id: Some(7),
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
                    row_ids: Vec::new(),
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
                    filed: None,
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

    #[gpui::test]
    fn stable_ids_rebind_notes_after_insert_and_delete(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let note = |row, row_id| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(row),
                row_id: Some(row_id),
                column: Some("Title".into()),
            };
            Diagnostics::set_note(note(1, 10), "ten".into(), cx);
            Diagnostics::set_note(note(3, 20), "twenty".into(), cx);

            Diagnostics::align_note_rows(DATASET_MAIN, &[99, 10, 11, 12, 20], cx);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(1), Some("Title"), cx),
                Some("ten".into())
            );
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(4), Some("Title"), cx),
                Some("twenty".into())
            );

            Diagnostics::align_note_rows(DATASET_MAIN, &[99, 11, 12, 20], cx);
            assert_eq!(
                Diagnostics::all(cx).len(),
                1,
                "deleted id 10 takes its note"
            );
            assert_eq!(Diagnostics::all(cx)[0].location.row, Some(3));

            Diagnostics::column_renamed(DATASET_MAIN, "Title", &"Caption".into(), cx);
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(3), Some("Caption"), cx),
                Some("twenty".into())
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
