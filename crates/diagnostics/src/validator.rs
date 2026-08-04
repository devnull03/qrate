//! The validator plugin boundary: a trait each validator crate implements, and the registry the
//! app fills at startup.
//!
//! Shaped after Neovim's `vim.diagnostic` and VS Code's `DiagnosticCollection`, which agree on the
//! rule this crate already implements: a producer publishes its *complete* set and that replaces
//! whatever it published before. Neither offers "add one diagnostic", and neither does
//! [`Validators::run`] — a re-run is the only invalidation there is.
//!
//! A validator never builds a [`Location`] or a [`Source`]. It reports `(row, severity, message)`
//! against one column and the registry addresses it, the same split as an LSP server reporting
//! ranges while the client owns the URI. That is what lets a validator live in its own crate
//! knowing nothing about datasets, projects, or the table.

use gpui::{App, BorrowAppContext as _, Global, SharedString};
use settings::columns::ColumnSettings;

use crate::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source};

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

/// One column-wise check. A `dyn` trait rather than an enum so a validator's dependencies — a
/// dictionary, a pattern set, an embedded Lua VM — stay out of every other crate's graph. The
/// plugin host implements it once, per loaded script, which is how a Lua file becomes a producer
/// indistinguishable from a compiled-in one.
pub trait ColumnValidator: 'static {
    /// What the Problems panel shows in the source column, and the key its output is replaced by.
    /// Must be stable across runs and unique across validators.
    fn name(&self) -> SharedString;

    /// Check one column top to bottom. `values` is every row's text for this column, in source-row
    /// order, so the returned index *is* the row. Returning nothing means the column is clean —
    /// which is also how a validator that does not apply here opts out.
    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)>;
}

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
    /// `columns` pairs each column's stable `c{ix}` settings key with its header name, in the same
    /// order as each row's cells. One [`Diagnostics::set`] per validator, carrying every column it
    /// flagged, so the replace-by-source rule makes the run self-invalidating: a fixed cell
    /// disappears because the next run simply doesn't report it.
    ///
    // ponytail: re-checks every column on every edit. Publishing per (source, column) instead
    // would need a finer replace key than `Diagnostics::set` has; add both together if a large
    // sheet starts stuttering on commit.
    pub fn run(columns: &[(SharedString, SharedString)], rows: &[Vec<SharedString>], cx: &mut App) {
        if cx.try_global::<Self>().is_none_or(|v| v.0.is_empty()) {
            return;
        }
        let settings = settings::columns::load(cx);
        // Owned, because the loop below needs `cx` mutably to publish.
        let types: Vec<String> = {
            let project = cx.try_global::<settings::project::CurrentProject>();
            columns
                .iter()
                .map(|(_, name)| {
                    project
                        .and_then(|p| p.data.columns.iter().find(|c| c.name == name.as_ref()))
                        .map_or(String::new(), |c| c.data_type.clone())
                })
                .collect()
        };
        let blank = ColumnSettings::default();
        // Transposed once, not once per validator: every validator wants the same column-major
        // view, and rebuilding it per validator is the whole sheet cloned again for each.
        let by_column: Vec<Vec<SharedString>> = (0..columns.len())
            .map(|ix| {
                rows.iter()
                    .map(|r| r.get(ix).cloned().unwrap_or_default())
                    .collect()
            })
            .collect();

        cx.update_global::<Self, _>(|this, cx| {
            for validator in &this.0 {
                let mut items = Vec::new();
                for (ix, (key, name)) in columns.iter().enumerate() {
                    let info = ColumnInfo {
                        name,
                        data_type: &types[ix],
                        settings: settings.get(key.as_ref()).unwrap_or(&blank),
                    };
                    items.extend(validator.validate(&info, &by_column[ix]).into_iter().map(
                        |(row, severity, message)| Diagnostic {
                            location: Location {
                                dataset: DATASET_MAIN.into(),
                                row: Some(row),
                                column: Some(name.clone()),
                            },
                            severity,
                            source: Source::Validator(validator.name()),
                            message,
                        },
                    ));
                }
                Diagnostics::set(
                    &Source::Validator(validator.name()),
                    DATASET_MAIN,
                    items,
                    cx,
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `lib.rs`'s test module.
    use crate::{
        ColumnInfo, ColumnValidator, DATASET_MAIN, Diagnostics, Severity, Source, Validators,
    };
    use gpui::{SharedString, TestAppContext};

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
            values: &[SharedString],
        ) -> Vec<(usize, Severity, SharedString)> {
            values
                .iter()
                .enumerate()
                .filter(|(_, v)| v.as_ref() == self.bad)
                .map(|(row, _)| {
                    (
                        row,
                        Severity::Error,
                        format!("{} in {}", self.bad, column.name).into(),
                    )
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
}
