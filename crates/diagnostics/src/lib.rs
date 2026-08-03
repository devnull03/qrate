//! Diagnostics: the app-wide problem list — imported cell notes today, validators, spell-check,
//! and user marks later — plus the bottom-dock [`ProblemsPanel`] that lists them.
//!
//! A leaf crate on purpose: it never depends on `table`, so `table` can depend on *it* for
//! in-cell squiggles later. The cost of that inversion is [`DiagnosticHooks`], through which the
//! panel asks the app to reveal a cell.

mod panel;
pub use panel::ProblemsPanel;

use std::path::PathBuf;

use gpui::{App, Global, Hsla, SharedString};
use gpui_component::ActiveTheme as _;

/// The one dataset a project can hold today. `__notes` keys by name so a second sheet is new
/// rows rather than a new table.
pub const DATASET_MAIN: &str = "dataset_main";

/// Where a problem points. `column` is the column *name*, not the table delegate's positional
/// `c{ix}` key: names are `__columns`' PRIMARY KEY and survive a column move.
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

/// How loud a problem is. Closed set — a user-authored mark is just a [`Severity::Note`] from
/// [`Source::User`], so marks and validator output share one list and one colour scale.
/// Ordered worst-first so sorting by it floats errors to the top.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Note,
}

impl Severity {
    /// Stable `__notes.severity` spelling. Unknown text reads back as [`Severity::Note`] — a
    /// project written by a newer qrate stays readable, just flattened.
    pub fn key(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Note => "note",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            "info" => Severity::Info,
            _ => Severity::Note,
        }
    }
}

/// What emitted a problem. Doubles as the invalidation key for [`Diagnostics::set`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// Carried in from the imported spreadsheet — xlsx cell notes today. Persists.
    Import,
    /// Authored in qrate by a person. Persists.
    User,
    /// A named rule, validator, plugin, or language server — the string is what the panel shows.
    /// Never persisted: computed output is recomputed on open, and stored copies go stale.
    Validator(SharedString),
}

impl Source {
    /// Provenance shown in the panel and stored in `__notes.source`.
    pub fn label(&self) -> SharedString {
        match self {
            Source::Import => "import".into(),
            Source::User => "you".into(),
            Source::Validator(name) => name.clone(),
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "import" => Source::Import,
            "you" => Source::User,
            other => Source::Validator(other.to_string().into()),
        }
    }

    /// Whether this source's output belongs in the `.qrate` file. Authored content persists;
    /// computed content is recomputed on open.
    pub fn persists(&self) -> bool {
        matches!(self, Source::Import | Source::User)
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
        Severity::Info => t.info,
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
    }

    /// Everything currently open, unsorted — the panel sorts and filters for display.
    pub fn all(cx: &App) -> &[Diagnostic] {
        cx.try_global::<Self>().map_or(&[], |d| d.items.as_slice())
    }

    /// Errors and warnings only, for the status bar's alert badge — a project full of imported
    /// notes should not light up a warning triangle.
    pub fn count(cx: &App) -> usize {
        Self::all(cx)
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
            .count()
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

    // Grouped by source so each keeps its own replace-by-source slot, exactly as if it had just
    // published: a later re-import of one source can't drop another's notes.
    for source in [Source::Import, Source::User] {
        let key = source.label();
        let items = stored
            .iter()
            .filter(|n| n.source == key.as_ref())
            .map(|n| Diagnostic {
                location: Location {
                    dataset: n.dataset.clone().into(),
                    row: n.row,
                    column: n.column.clone().map(SharedString::from),
                },
                severity: Severity::from_key(&n.severity),
                source: source.clone(),
                message: n.message.clone().into(),
            })
            .collect();
        Diagnostics::set(&source, DATASET_MAIN, items, cx);
    }
}

/// Set once at startup (see `crates/app/src/main.rs`) so the Problems panel can jump the table to
/// a cell without this crate depending on `table` — that edge has to stay free for
/// `table -> diagnostics` (in-cell squiggles) later.
#[derive(Clone, Copy)]
pub struct DiagnosticHooks {
    pub reveal: fn(&Location, &mut App),
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
            "import",
            &[
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(2),
                    column: Some("Title".into()),
                    severity: "warning".into(),
                    source: "import".into(),
                    message: "check this".into(),
                },
                StoredNote {
                    dataset: DATASET_MAIN.into(),
                    row: Some(4),
                    column: None,
                    severity: "note".into(),
                    source: "import".into(),
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
            assert_eq!(Diagnostics::count(cx), 1, "only the warning is countable");

            // Severity and the optional coordinates survive the disk round-trip, and the source
            // comes back as `Import` so a re-import replaces exactly these.
            let wide = all.iter().find(|d| d.message == "whole row").unwrap();
            assert_eq!(wide.severity, Severity::Note);
            assert_eq!((wide.location.row, &wide.location.column), (Some(4), &None));
            assert!(all.iter().all(|d| d.source == Source::Import));

            // Re-running is idempotent: the `loaded` guard stops the same project's notes being
            // read again on every project-scoped setting write.
            init(cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);
        });
    }

    #[gpui::test]
    fn publishing_replaces_only_that_sources_diagnostics(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let import = |n: usize| {
                (0..n)
                    .map(|i| {
                        diag(
                            Severity::Note,
                            Source::Import,
                            DATASET_MAIN,
                            &format!("n{i}"),
                        )
                    })
                    .collect::<Vec<_>>()
            };

            Diagnostics::set(&Source::Import, DATASET_MAIN, import(2), cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);

            // Republishing replaces rather than appends — 1, not 3.
            Diagnostics::set(&Source::Import, DATASET_MAIN, import(1), cx);
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
            Diagnostics::set(&Source::Import, DATASET_MAIN, vec![], cx);
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
    fn the_status_count_ignores_notes_and_info(cx: &mut TestAppContext) {
        cx.update(|cx| {
            Diagnostics::set(
                &Source::Import,
                DATASET_MAIN,
                vec![
                    diag(Severity::Error, Source::Import, DATASET_MAIN, "e"),
                    diag(Severity::Warning, Source::Import, DATASET_MAIN, "w"),
                    diag(Severity::Info, Source::Import, DATASET_MAIN, "i"),
                    diag(Severity::Note, Source::Import, DATASET_MAIN, "n"),
                ],
                cx,
            );
            assert_eq!(Diagnostics::all(cx).len(), 4);
            assert_eq!(Diagnostics::count(cx), 2);
        });
    }
}
