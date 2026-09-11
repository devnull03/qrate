//! The validator plugin boundary: a trait each validator crate implements, and the registry the
//! app fills at startup.
//!
//! Shaped after Neovim's `vim.diagnostic` and VS Code's `DiagnosticCollection`, which agree on the
//! rule this crate already implements: a producer publishes its *complete* set and that replaces
//! whatever it published before. Neither offers "add one diagnostic", and neither does
//! [`Validators::run`] — a re-run is the only invalidation there is.
//!
//! A column validator never builds a [`Location`] or a [`Source`]. It reports [`ColumnFinding`] values
//! against one column and the registry addresses it, the same split as an LSP server reporting
//! ranges while the client owns the URI. That is what lets a validator live in its own crate
//! knowing nothing about datasets, projects, or the table.

use std::collections::BTreeMap;

use gpui::{App, BorrowAppContext as _, Global, SharedString};
use settings::columns::ColumnSettings;

use crate::{DATASET_MAIN, Diagnostic, DiagnosticGroup, Diagnostics, Location, Severity, Source};

/// One column's whole input, owned. A validator that runs later — off the UI thread, after this
/// run's borrows are gone — needs the data to outlive the call, which [`ColumnInfo`] cannot do.
#[derive(Clone)]
pub struct ColumnSnapshot {
    pub name: SharedString,
    pub data_type: SharedString,
    pub settings: ColumnSettings,
    pub values: Vec<SharedString>,
    pub subdelimiter: SharedString,
}

impl ColumnSnapshot {
    pub fn info(&self) -> ColumnInfo<'_> {
        ColumnInfo {
            name: &self.name,
            data_type: &self.data_type,
            settings: &self.settings,
        }
    }
}

/// The column being checked. Everything a validator is allowed to know about where its values
/// came from.
pub struct ColumnInfo<'a> {
    /// Header text. Also the name the resulting diagnostics are filed under.
    pub name: &'a str,
    /// The column's declared type from the project's `__columns`, or empty if unconfigured.
    pub data_type: &'a str,
    /// This column's per-project preferences, which is where a validator's own knobs live.
    pub settings: &'a ColumnSettings,
}

#[derive(Clone, Copy)]
pub struct ColumnValues<'a> {
    raw: &'a [SharedString],
    subdelimiter: &'a str,
}

impl<'a> ColumnValues<'a> {
    pub fn new(raw: &'a [SharedString], subdelimiter: &'a str) -> Self {
        Self { raw, subdelimiter }
    }

    pub fn raw(self) -> &'a [SharedString] {
        self.raw
    }

    pub fn iter(self) -> impl Iterator<Item = CellValue<'a>> {
        self.raw
            .iter()
            .enumerate()
            .map(move |(row, raw)| CellValue {
                row,
                raw,
                subdelimiter: self.subdelimiter,
            })
    }

    pub fn replace_part(
        self,
        row: usize,
        observed: &str,
        replacement: &str,
    ) -> Option<SharedString> {
        let raw = self.raw.get(row)?.as_ref();
        for (offset, _) in raw.match_indices(observed) {
            let end = offset + observed.len();
            let bounded = if self.subdelimiter.is_empty() {
                raw.trim() == observed
            } else {
                (raw[..offset].trim_end().ends_with(self.subdelimiter)
                    || raw[..offset].trim().is_empty())
                    && (raw[end..].trim_start().starts_with(self.subdelimiter)
                        || raw[end..].trim().is_empty())
            };
            if bounded {
                let mut result = raw.to_owned();
                result.replace_range(offset..end, replacement);
                return Some(result.into());
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
pub struct CellValue<'a> {
    pub row: usize,
    pub raw: &'a str,
    subdelimiter: &'a str,
}

impl<'a> CellValue<'a> {
    pub fn parts(self) -> ValueParts<'a> {
        if self.subdelimiter.is_empty() {
            ValueParts::Whole(
                (!self.raw.trim().is_empty())
                    .then_some(self.raw.trim())
                    .into_iter(),
            )
        } else {
            ValueParts::Split(self.raw.split(self.subdelimiter))
        }
    }
}

pub enum ValueParts<'a> {
    Whole(std::option::IntoIter<&'a str>),
    Split(std::str::Split<'a, &'a str>),
}

impl<'a> Iterator for ValueParts<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ValueParts::Whole(values) => values.next(),
            ValueParts::Split(values) => values.find_map(|value| {
                let value = value.trim();
                (!value.is_empty()).then_some(value)
            }),
        }
    }
}

/// `None` addresses the whole column; `Some` addresses a source-row cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnFinding {
    pub row: Option<usize>,
    pub severity: Severity,
    pub message: SharedString,
    pub group: Option<DiagnosticGroup>,
}

impl From<(usize, Severity, SharedString)> for ColumnFinding {
    fn from((row, severity, message): (usize, Severity, SharedString)) -> Self {
        Self {
            row: Some(row),
            severity,
            message,
            group: None,
        }
    }
}

/// One column-wise check. A `dyn` trait rather than an enum so a validator's dependencies — a
/// dictionary, a pattern set, an embedded Lua VM — stay out of every other crate's graph. The
/// plugin host implements it once, per loaded script, which is how a Lua file becomes a producer
/// indistinguishable from a compiled-in one.
pub trait ColumnValidator: 'static {
    /// What the Problems panel shows in the source column, and the key its output is replaced by.
    /// Must be stable across runs and unique across validators.
    fn name(&self) -> SharedString;

    /// Clear snapshot-bound state, including columns removed since the previous run.
    fn begin_run(&self) {}

    /// Check one column top to bottom. `values` is every row's text for this column, in source-row
    /// order, so the returned index *is* the row. Returning nothing means the column is clean —
    /// which is also how a validator that does not apply here opts out.
    fn validate(&self, column: &ColumnInfo, values: ColumnValues<'_>) -> Vec<ColumnFinding>;
}

/// Producers that cannot answer while the run is on the stack — the plugin host running its VMs,
/// an authority checked over the network, a files folder walked from disk. Reached through
/// function pointers for the same reason `DiagnosticHooks` is: the caller must not link the
/// producer.
///
/// A list rather than one slot: each of those publishes under its own [`Source`], so they compose
/// the way registered [`ColumnValidator`]s do instead of overwriting each other.
#[derive(Default)]
pub struct AsyncValidators(BTreeMap<SharedString, fn(&[ColumnSnapshot], &mut App)>);

impl Global for AsyncValidators {}

impl AsyncValidators {
    /// Keyed by producer name so re-registering replaces rather than doubles: the plugin host
    /// re-runs this every time a plugin is switched on or off, and running its VMs twice per edit
    /// is the bug that shape invites.
    pub fn register(name: &str, run: fn(&[ColumnSnapshot], &mut App), cx: &mut App) {
        cx.default_global::<Self>().0.insert(name.into(), run);
    }
}

/// One misspelled word, and the corrections offered for it in rank order.
pub type Misspelling = (SharedString, Vec<SharedString>);

/// The two things a spell checker can offer that reading a [`Diagnostic`](crate::Diagnostic)
/// cannot: what a misspelled word should have been, and a way to accept it.
///
/// Function pointers for the same reason [`AsyncValidators`] is one — the table builds the
/// right-click menu and must not link the dictionary. Asked when a menu opens rather than carried
/// on each diagnostic, which keeps a diagnostic's message a sentence for a human instead of a
/// format the table has to parse back.
#[derive(Clone, Copy)]
pub struct SpellActions {
    /// Every misspelled word in the text, each with its ranked suggestions, in first-seen order.
    pub suggest: fn(&str, &App) -> Vec<Misspelling>,
    /// Accept a word permanently, and re-run validation so its findings clear.
    pub add_word: fn(&str, &mut App),
}

impl Global for SpellActions {}

/// Every registered validator. Filled by `app` at startup, which is the only place that knows
/// where validators come from.
#[derive(Default)]
pub struct Validators(Vec<Box<dyn ColumnValidator>>);

impl Global for Validators {}

impl Validators {
    pub fn register(validator: Box<dyn ColumnValidator>, cx: &mut App) {
        cx.default_global::<Self>().0.push(validator);
    }

    /// Drop a validator and clear what it published. Publishing an empty set is the only
    /// invalidation this store has, so removal has to do it explicitly — nothing else ever will,
    /// since the validator is gone before the next run.
    pub fn remove(name: &SharedString, cx: &mut App) {
        Diagnostics::set(
            &Source::Validator(name.clone()),
            DATASET_MAIN,
            Vec::new(),
            cx,
        );
        cx.default_global::<Self>().0.retain(|v| &v.name() != name);
    }

    /// Run every validator over every column and publish the results.
    ///
    /// `columns` pairs each column's settings key with its header name (today the same string,
    /// kept as a pair so a validator never has to know that), in the same
    /// order as each row's cells. One [`Diagnostics::set`] per validator, carrying every column it
    /// flagged, so the replace-by-source rule makes the run self-invalidating: a fixed cell
    /// disappears because the next run simply doesn't report it.
    ///
    pub fn run(columns: &[(SharedString, SharedString)], rows: &[Vec<SharedString>], cx: &mut App) {
        let sync = cx.try_global::<Self>().is_some_and(|v| !v.0.is_empty());
        // Copied out because running one hands `cx` back mutably, and a fn pointer is cheap.
        let deferred: Vec<_> = cx
            .try_global::<AsyncValidators>()
            .map(|v| v.0.values().copied().collect())
            .unwrap_or_default();
        if !sync && deferred.is_empty() {
            return;
        }

        let settings = settings::columns::load(cx);
        let subdelimiter = if cx.has_global::<settings::AppSettings>() {
            settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx)
        } else {
            SharedString::default()
        };
        let project = cx.try_global::<settings::project::CurrentProject>();
        let blank = ColumnSettings::default();
        // Transposed once, not once per validator: every validator wants the same column-major
        // view, and rebuilding it per validator is the whole sheet cloned again for each.
        let snapshot: Vec<ColumnSnapshot> = columns
            .iter()
            .enumerate()
            .map(|(ix, (key, name))| ColumnSnapshot {
                name: name.clone(),
                data_type: project
                    .and_then(|p| p.data.columns.iter().find(|c| c.name == name.as_ref()))
                    .map_or_else(SharedString::default, |c| c.data_type.clone().into()),
                settings: settings.get(key.as_ref()).unwrap_or(&blank).clone(),
                values: rows
                    .iter()
                    .map(|r| r.get(ix).cloned().unwrap_or_default())
                    .collect(),
                subdelimiter: subdelimiter.clone(),
            })
            .collect();

        if sync {
            cx.update_global::<Self, _>(|this, cx| {
                for validator in &this.0 {
                    validator.begin_run();
                    let items = snapshot
                        .iter()
                        .flat_map(|column| {
                            address(
                                validator.name(),
                                column,
                                validator.validate(
                                    &column.info(),
                                    ColumnValues::new(&column.values, &column.subdelimiter),
                                ),
                            )
                        })
                        .collect();
                    Diagnostics::set(
                        &Source::Validator(validator.name()),
                        DATASET_MAIN,
                        items,
                        cx,
                    );
                }
            });
        }
        for run in deferred {
            run(&snapshot, cx);
        }
    }
}

/// Turn one validator's cell or column reports into addressed diagnostics. A validator
/// never builds a [`Location`] or a [`Source`]; this is where that split is honoured, and it is
/// public so a deferred producer addresses its findings identically.
///
/// It is also where [`ColumnSettings::severity`] is applied. Every producer — compiled-in, network,
/// and plugin alike — reaches diagnostics through here, so the override lands once instead of each
/// check having to read the setting and remember to honour it.
pub fn address(
    validator: SharedString,
    column: &ColumnSnapshot,
    found: Vec<ColumnFinding>,
) -> Vec<Diagnostic> {
    let override_to = column
        .settings
        .severity
        .get(validator.as_ref())
        .map(|key| Severity::from_key(key));
    found
        .into_iter()
        .map(|finding| Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: finding.row,
                row_id: None,
                column: Some(column.name.clone()),
            },
            severity: override_to.unwrap_or(finding.severity),
            source: Source::Validator(validator.clone()),
            message: finding.message,
            group: finding.group,
            filed: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `lib.rs`'s test module.
    use crate::{
        ColumnInfo, ColumnValidator, DATASET_MAIN, Diagnostics, Severity, Source, Validators,
    };
    use gpui::{SharedString, TestAppContext};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn column_values_borrow_raw_cells_and_split_without_per_cell_storage() {
        let raw = [
            SharedString::from("Busson, Carl W.| Dhillon, Baltej Singh "),
            SharedString::from(""),
        ];
        let values = crate::ColumnValues::new(&raw, "|");
        let cells: Vec<_> = values.iter().collect();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].row, 0);
        assert_eq!(
            cells[0].parts().collect::<Vec<_>>(),
            ["Busson, Carl W.", "Dhillon, Baltej Singh"]
        );
        assert!(cells[1].parts().next().is_none());
        assert_eq!(
            values.replace_part(0, "Busson, Carl W.", "Carl W. Busson"),
            Some("Carl W. Busson| Dhillon, Baltej Singh ".into())
        );
        assert!(values.replace_part(0, "Carl", "wrong boundary").is_none());
    }

    /// Flags any cell equal to `bad`, so a test can steer exactly how many items a run produces.
    struct Flag {
        name: &'static str,
        bad: &'static str,
    }

    impl ColumnValidator for Flag {
        fn name(&self) -> SharedString {
            self.name.into()
        }

        fn validate(
            &self,
            column: &ColumnInfo,
            values: crate::ColumnValues<'_>,
        ) -> Vec<super::ColumnFinding> {
            values
                .iter()
                .filter(|value| value.raw == self.bad)
                .map(|value| {
                    (
                        value.row,
                        Severity::Error,
                        format!("{} in {}", self.bad, column.name).into(),
                    )
                        .into()
                })
                .collect()
        }
    }

    fn grid() -> (Vec<(SharedString, SharedString)>, Vec<Vec<SharedString>>) {
        (
            vec![
                ("c0".into(), "Title".into()),
                ("c1".into(), "Format".into()),
            ],
            vec![
                vec!["ok".into(), "bad".into()],
                vec!["bad".into(), "ok".into()],
            ],
        )
    }

    #[gpui::test]
    fn a_run_addresses_and_publishes_what_validators_report(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (columns, rows) = grid();
            Validators::register(
                Box::new(Flag {
                    name: "flag",
                    bad: "bad",
                }),
                cx,
            );
            Validators::run(&columns, &rows, cx);

            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 2);
            // The validator returned only a row index; the registry supplied dataset, column, and
            // source — which is the whole point of the split.
            let title = all
                .iter()
                .find(|d| d.location.column.as_deref() == Some("Title"))
                .expect("the Title column's finding is addressed by name");
            assert_eq!(title.location.row, Some(1));
            assert_eq!(title.location.dataset, DATASET_MAIN);
            assert_eq!(title.source, Source::Validator("flag".into()));
        });
    }

    #[gpui::test]
    fn a_re_run_replaces_only_its_own_validators_output(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (columns, rows) = grid();
            Validators::register(
                Box::new(Flag {
                    name: "flag",
                    bad: "bad",
                }),
                cx,
            );
            Validators::register(
                Box::new(Flag {
                    name: "other",
                    bad: "ok",
                }),
                cx,
            );
            Validators::run(&columns, &rows, cx);
            assert_eq!(Diagnostics::all(cx).len(), 4);

            // The user fixes both "bad" cells. `flag` now finds nothing, and publishing nothing is
            // what clears its stale entries; `other` is republished independently.
            let fixed = vec![
                vec!["ok".into(), "ok".into()],
                vec!["ok".into(), "ok".into()],
            ];
            Validators::run(&columns, &fixed, cx);
            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 4, "`other` now matches all four cells");
            assert!(
                all.iter()
                    .all(|d| d.source == Source::Validator("other".into())),
                "`flag`'s findings cleared themselves by not being republished"
            );
        });
    }

    #[gpui::test]
    fn removing_a_validator_clears_its_findings_and_leaves_the_rest(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (columns, rows) = grid();
            Validators::register(
                Box::new(Flag {
                    name: "flag",
                    bad: "bad",
                }),
                cx,
            );
            Validators::register(
                Box::new(Flag {
                    name: "other",
                    bad: "ok",
                }),
                cx,
            );
            Validators::run(&columns, &rows, cx);
            assert_eq!(Diagnostics::all(cx).len(), 4);

            Validators::remove(&"flag".into(), cx);
            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 2, "removal clears without waiting for a run");
            assert!(
                all.iter()
                    .all(|d| d.source == Source::Validator("other".into()))
            );

            // And it stays gone: the next run has nothing left to republish it.
            Validators::run(&columns, &rows, cx);
            assert_eq!(Diagnostics::all(cx).len(), 2);
        });
    }

    #[gpui::test]
    fn a_run_without_registered_validators_touches_nothing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (columns, rows) = grid();
            Validators::run(&columns, &rows, cx);
            assert!(Diagnostics::all(cx).is_empty());
        });
    }

    /// A deferred producer publishes through the hook rather than the registry — so an empty
    /// registry must not skip the run, or nothing validates in the shipping app.
    #[gpui::test]
    fn a_deferred_producer_runs_with_an_empty_registry(cx: &mut TestAppContext) {
        static SEEN: AtomicUsize = AtomicUsize::new(0);
        fn record(columns: &[crate::ColumnSnapshot], _: &mut gpui::App) {
            SEEN.store(columns.len(), Ordering::SeqCst);
        }

        cx.update(|cx| {
            let (columns, rows) = grid();
            crate::AsyncValidators::register("test", record, cx);
            Validators::run(&columns, &rows, cx);
            assert_eq!(SEEN.load(Ordering::SeqCst), 2, "both columns crossed over");
        });
    }

    fn snapshot(overrides: &[(&str, &str)]) -> crate::ColumnSnapshot {
        let mut settings = settings::columns::ColumnSettings::default();
        for (producer, severity) in overrides {
            settings
                .severity
                .insert(producer.to_string(), severity.to_string());
        }
        crate::ColumnSnapshot {
            name: "Photographer".into(),
            data_type: "Text".into(),
            settings,
            values: vec!["Aderman, Ray".into()],
            subdelimiter: SharedString::default(),
        }
    }

    /// The whole point of putting the override in `address`: a check keeps reporting `Error` and
    /// the column's setting is what decides how loud that lands.
    #[test]
    fn a_columns_override_replaces_the_severity_the_check_reported() {
        let found = vec![(0, Severity::Error, SharedString::from("not in LCSH")).into()];
        let addressed = crate::address("LCSH".into(), &snapshot(&[("LCSH", "warning")]), found);
        assert_eq!(addressed[0].severity, Severity::Warning);
    }

    #[test]
    fn column_findings_preserve_scope_and_group_under_overrides() {
        let group = crate::DiagnosticGroup {
            key: "missing".into(),
            summary: "Missing values".into(),
            subject: None,
        };
        let found = vec![super::ColumnFinding {
            row: None,
            severity: Severity::Error,
            message: "Column is empty".into(),
            group: Some(group.clone()),
        }];
        let addressed = crate::address(
            "required".into(),
            &snapshot(&[("required", "warning")]),
            found,
        );
        assert_eq!(
            addressed[0].location.scope(),
            crate::Scope::Column("Photographer")
        );
        assert_eq!(addressed[0].severity, Severity::Warning);
        assert_eq!(addressed[0].group, Some(group));
        assert_eq!(crate::Location::dataset("d").scope(), crate::Scope::Dataset);
        assert_eq!(
            crate::Location::row("d", 4, None).scope(),
            crate::Scope::Row(4)
        );
        assert_eq!(
            crate::Location::cell("d", 4, None, "c").scope(),
            crate::Scope::Cell {
                row: 4,
                column: "c"
            }
        );
    }

    /// An override names one producer, so it must not quiet the others checking the same column.
    #[test]
    fn an_override_for_one_producer_leaves_the_rest_alone() {
        let column = snapshot(&[("LCSH", "warning")]);
        let found: Vec<super::ColumnFinding> =
            vec![(0, Severity::Error, SharedString::from("no such file")).into()];
        let addressed = crate::address("files".into(), &column, found.clone());
        assert_eq!(addressed[0].severity, Severity::Error);
        // And a column with nothing overridden is untouched either way.
        let addressed = crate::address("files".into(), &snapshot(&[]), found);
        assert_eq!(addressed[0].severity, Severity::Error);
    }

    /// `plugin_host::reload` re-registers on every plugin toggle. Keyed by name, so that replaces
    /// its entry — a registry that appended would run every VM twice per edit after one toggle.
    #[gpui::test]
    fn re_registering_a_name_replaces_it(cx: &mut TestAppContext) {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        fn count(_: &[crate::ColumnSnapshot], _: &mut gpui::App) {
            RUNS.fetch_add(1, Ordering::SeqCst);
        }

        cx.update(|cx| {
            let (columns, rows) = grid();
            crate::AsyncValidators::register("plugins", count, cx);
            crate::AsyncValidators::register("plugins", count, cx);
            Validators::run(&columns, &rows, cx);
            assert_eq!(RUNS.load(Ordering::SeqCst), 1, "registered twice, ran once");

            // A different name is a different producer and does get its own run.
            crate::AsyncValidators::register("files", count, cx);
            Validators::run(&columns, &rows, cx);
            assert_eq!(RUNS.load(Ordering::SeqCst), 3);
        });
    }
}
