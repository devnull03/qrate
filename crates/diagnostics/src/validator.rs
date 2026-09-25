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

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gpui::{App, AppContext as _, Global, SharedString};
use settings::columns::ColumnSettings;

use crate::{DATASET_MAIN, Diagnostic, DiagnosticGroup, Diagnostics, Location, Severity, Source};

/// One column's whole input, owned. A validator that runs later — off the UI thread, after this
/// run's borrows are gone — needs the data to outlive the call, which [`ColumnInfo`] cannot do.
///
/// Cheap to clone: a column that did not change since the previous run shares that run's values
/// and settings, and keeps its `revision`.
#[derive(Clone)]
pub struct ColumnSnapshot {
    pub name: SharedString,
    pub data_type: SharedString,
    pub settings: Arc<ColumnSettings>,
    pub values: Arc<[SharedString]>,
    pub subdelimiter: SharedString,
    pub row_ids: Arc<[settings::project::RowId]>,
    /// Equal across two runs only when every field above is, so a producer can reuse what it
    /// found in this column last time.
    pub revision: u64,
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
pub trait ColumnValidator: Send + Sync + 'static {
    /// What the Problems panel shows in the source column, and the key its output is replaced by.
    /// Must be stable across runs and unique across validators.
    fn name(&self) -> SharedString;

    /// Drop snapshot-bound state for columns not in `columns`. Only changed columns are validated
    /// again, so state for the others must survive.
    fn begin_run(&self, _columns: &[SharedString]) {}

    /// Changes when this validator would answer differently for an unchanged column.
    fn revision(&self) -> u64 {
        0
    }

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

const SLOW_PUBLISH: Duration = Duration::from_millis(16);

/// Column name → the column revision and validator revision a finding set was made for.
type Found = HashMap<SharedString, (u64, u64, Vec<Diagnostic>)>;

/// `(column, column revision, validator revision)` for every column one publish covered.
type Covered = Vec<(SharedString, u64, u64)>;

struct Registered {
    validator: Arc<dyn ColumnValidator>,
    /// Reused for every column whose revision is unchanged, so an edit re-checks one column.
    found: Arc<Mutex<Found>>,
    /// What the store holds for this validator. A run covering the same thing publishes nothing,
    /// which spares every observer of the store a refresh.
    published: Covered,
}

/// Every registered validator. Filled by `app` at startup, which is the only place that knows
/// where validators come from.
#[derive(Default)]
pub struct Validators {
    registered: Vec<Registered>,
    /// Bumped by every run and removal, so a pass that finishes after a newer one started — or
    /// after its validator was dropped — publishes nothing.
    generation: Arc<AtomicU64>,
    /// Held for a whole background pass: validators keep per-run state (`begin_run`), which two
    /// interleaved passes would mix.
    running: Arc<Mutex<()>>,
    /// The previous run's snapshot, which the next one compares against column by column.
    snapshot: Option<Arc<[ColumnSnapshot]>>,
    revisions: u64,
}

impl Global for Validators {}

impl Validators {
    pub fn register(validator: Box<dyn ColumnValidator>, cx: &mut App) {
        cx.default_global::<Self>().registered.push(Registered {
            validator: Arc::from(validator),
            found: Arc::default(),
            published: Vec::new(),
        });
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
        let this = cx.default_global::<Self>();
        this.generation.fetch_add(1, Ordering::SeqCst);
        this.registered.retain(|r| &r.validator.name() != name);
    }

    /// Run every validator over every changed column and publish the results.
    ///
    /// `columns` pairs each column's settings key with its header name (today the same string,
    /// kept as a pair so a validator never has to know that), in the same
    /// order as each row's cells. One [`Diagnostics::set`] per validator, carrying every column it
    /// flagged, so the replace-by-source rule makes the run self-invalidating: a fixed cell
    /// disappears because the next run simply doesn't report it.
    ///
    /// A column whose text, settings, type and rows all match the previous run keeps its
    /// [`ColumnSnapshot::revision`], and each validator reuses what it found there. Renaming,
    /// adding, removing or reordering a column, or any change of row ids, re-checks every column.
    ///
    /// Only the snapshot is taken on the UI thread. The checking runs on the background executor
    /// and lands a moment later; a newer run supersedes one that has not landed yet.
    pub fn run(
        columns: &[(SharedString, SharedString)],
        rows: &[Vec<SharedString>],
        row_ids: &[settings::project::RowId],
        cx: &mut App,
    ) {
        Diagnostics::set_row_ids(row_ids, cx);
        let validators: Vec<_> = cx.try_global::<Self>().map_or_else(Vec::new, |v| {
            v.registered
                .iter()
                .map(|r| (r.validator.clone(), r.found.clone()))
                .collect()
        });
        // Copied out because running one hands `cx` back mutably, and a fn pointer is cheap.
        let deferred: Vec<_> = cx
            .try_global::<AsyncValidators>()
            .map(|v| v.0.iter().map(|(name, run)| (name.clone(), *run)).collect())
            .unwrap_or_default();
        if validators.is_empty() && deferred.is_empty() {
            return;
        }

        let started = Instant::now();
        let (previous, mut revisions) = cx
            .try_global::<Self>()
            .map_or((None, 0), |v| (v.snapshot.clone(), v.revisions));
        let previous: Arc<[ColumnSnapshot]> = previous
            .filter(|before| {
                before.len() == columns.len()
                    && before
                        .iter()
                        .zip(columns)
                        .all(|(column, (_, name))| column.name == *name)
                    && before
                        .first()
                        .is_none_or(|column| *column.row_ids == *row_ids)
            })
            .unwrap_or_else(|| Arc::from([]));
        let row_ids: Arc<[_]> = previous
            .first()
            .map_or_else(|| row_ids.into(), |column| column.row_ids.clone());
        let settings = settings::columns::shared(cx);
        let subdelimiter = if cx.has_global::<settings::AppSettings>() {
            settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx)
        } else {
            SharedString::default()
        };
        let project = cx.try_global::<settings::project::CurrentProject>();
        let (blank, empty) = (ColumnSettings::default(), SharedString::default());
        let same = |a: &SharedString, b: &SharedString| {
            (a.as_ptr() == b.as_ptr() && a.len() == b.len()) || a == b
        };
        // Transposed once, not once per validator: every validator wants the same column-major
        // view, and rebuilding it per validator is the whole sheet cloned again for each.
        let snapshot: Arc<[ColumnSnapshot]> = columns
            .iter()
            .enumerate()
            .map(|(ix, (key, name))| {
                let before = previous.get(ix);
                let data_type = project
                    .and_then(|p| p.data.columns.iter().find(|c| c.name == name.as_ref()))
                    .map_or_else(SharedString::default, |c| c.data_type.clone().into());
                let wanted = settings.get(key.as_ref()).unwrap_or(&blank);
                let cells = || rows.iter().map(|r| r.get(ix).unwrap_or(&empty));
                let values = match before {
                    Some(before)
                        if before.values.len() == rows.len()
                            && cells().zip(before.values.iter()).all(|(a, b)| same(a, b)) =>
                    {
                        before.values.clone()
                    }
                    _ => cells().cloned().collect(),
                };
                let column_settings = match before {
                    Some(before) if *before.settings == *wanted => before.settings.clone(),
                    _ => Arc::new(wanted.clone()),
                };
                let revision = match before {
                    Some(before)
                        if Arc::ptr_eq(&before.values, &values)
                            && Arc::ptr_eq(&before.settings, &column_settings)
                            && before.data_type == data_type
                            && before.subdelimiter == subdelimiter =>
                    {
                        before.revision
                    }
                    _ => {
                        revisions += 1;
                        revisions
                    }
                };
                ColumnSnapshot {
                    name: name.clone(),
                    data_type,
                    settings: column_settings,
                    values,
                    subdelimiter: subdelimiter.clone(),
                    row_ids: row_ids.clone(),
                    revision,
                }
            })
            .collect();
        let this = cx.default_global::<Self>();
        this.snapshot = Some(snapshot.clone());
        this.revisions = revisions;

        for (name, run) in deferred {
            let step = Instant::now();
            run(&snapshot, cx);
            log::debug!("deferred validator {name:?} queued in {:?}", step.elapsed());
        }
        log::debug!(
            "validation snapshot: {} columns × {} rows taken in {:?}",
            columns.len(),
            rows.len(),
            started.elapsed()
        );
        if validators.is_empty() {
            return;
        }

        let this = cx.default_global::<Self>();
        let generation = this.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let latest = this.generation.clone();
        let running = this.running.clone();
        let current = move || latest.load(Ordering::SeqCst) == generation;
        let names: Vec<SharedString> = snapshot.iter().map(|c| c.name.clone()).collect();
        cx.spawn(async move |cx| {
            let still_current = current.clone();
            let found = cx
                .background_spawn(async move {
                    let _pass = running.lock().unwrap_or_else(PoisonError::into_inner);
                    if !still_current() {
                        return None;
                    }
                    let started = Instant::now();
                    let found: Vec<_> = validators
                        .iter()
                        .map(|(validator, found)| {
                            let step = Instant::now();
                            let name = validator.name();
                            validator.begin_run(&names);
                            let revision = validator.revision();
                            let mut found = found.lock().unwrap_or_else(PoisonError::into_inner);
                            found.retain(|column, _| names.contains(column));
                            let (mut items, mut covered, mut checked) =
                                (Vec::new(), Vec::with_capacity(snapshot.len()), 0);
                            for column in snapshot.iter() {
                                covered.push((column.name.clone(), column.revision, revision));
                                if let Some((_, _, cached)) = found.get(&column.name).filter(
                                    |(at, by, _)| *at == column.revision && *by == revision,
                                ) {
                                    items.extend(cached.iter().cloned());
                                    continue;
                                }
                                checked += 1;
                                let fresh = address(
                                    name.clone(),
                                    column,
                                    validator.validate(
                                        &column.info(),
                                        ColumnValues::new(&column.values, &column.subdelimiter),
                                    ),
                                );
                                items.extend(fresh.iter().cloned());
                                found.insert(
                                    column.name.clone(),
                                    (column.revision, revision, fresh),
                                );
                            }
                            log::debug!(
                                "validator {name:?}: {} findings, {checked} of {} columns checked in {:?}",
                                items.len(),
                                snapshot.len(),
                                step.elapsed()
                            );
                            (name, covered, items)
                        })
                        .collect();
                    log::debug!(
                        "validation checked off the UI thread in {:?}",
                        started.elapsed()
                    );
                    Some(found)
                })
                .await;
            let Some(found) = found else {
                return;
            };
            cx.update(|cx| {
                if !current() {
                    return;
                }
                let started = Instant::now();
                let this = cx.default_global::<Self>();
                let batch: Vec<_> = found
                    .into_iter()
                    .filter_map(|(name, covered, items)| {
                        let entry = this
                            .registered
                            .iter_mut()
                            .find(|r| r.validator.name() == name)?;
                        if entry.published == covered {
                            return None;
                        }
                        entry.published = covered;
                        Some((Source::Validator(name), items))
                    })
                    .collect();
                Diagnostics::set_many(DATASET_MAIN, batch, cx);
                let elapsed = started.elapsed();
                if elapsed >= SLOW_PUBLISH {
                    log::warn!("publishing validation blocked the UI thread for {elapsed:?}");
                } else {
                    log::debug!("validation published in {elapsed:?}");
                }
            });
        })
        .detach();
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
                row_id: finding.row.and_then(|row| column.row_ids.get(row).copied()),
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

    fn register(cx: &mut TestAppContext, validators: &[(&'static str, &'static str)]) {
        cx.update(|cx| {
            for &(name, bad) in validators {
                Validators::register(Box::new(Flag { name, bad }), cx);
            }
        });
    }

    /// A run only takes the snapshot; the findings land once the background pass is drained.
    fn run(cx: &mut TestAppContext, rows: &[Vec<SharedString>]) {
        cx.update(|cx| Validators::run(&grid().0, rows, &[], cx));
        cx.run_until_parked();
    }

    fn fixed() -> Vec<Vec<SharedString>> {
        vec![
            vec!["ok".into(), "ok".into()],
            vec!["ok".into(), "ok".into()],
        ]
    }

    #[gpui::test]
    fn a_run_addresses_and_publishes_what_validators_report(cx: &mut TestAppContext) {
        register(cx, &[("flag", "bad")]);
        run(cx, &grid().1);
        cx.update(|cx| {
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
        register(cx, &[("flag", "bad"), ("other", "ok")]);
        run(cx, &grid().1);
        cx.update(|cx| assert_eq!(Diagnostics::all(cx).len(), 4));

        // The user fixes both "bad" cells. `flag` now finds nothing, and publishing nothing is
        // what clears its stale entries; `other` is republished independently.
        run(cx, &fixed());
        cx.update(|cx| {
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
        register(cx, &[("flag", "bad"), ("other", "ok")]);
        run(cx, &grid().1);
        cx.update(|cx| {
            assert_eq!(Diagnostics::all(cx).len(), 4);
            Validators::remove(&"flag".into(), cx);
            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 2, "removal clears without waiting for a run");
            assert!(
                all.iter()
                    .all(|d| d.source == Source::Validator("other".into()))
            );
        });

        // And it stays gone: the next run has nothing left to republish it.
        run(cx, &grid().1);
        cx.update(|cx| assert_eq!(Diagnostics::all(cx).len(), 2));
    }

    /// Two runs queued before either lands: only the newer snapshot may publish, or a slow pass
    /// over old data would overwrite the answer for the data the user is looking at.
    #[gpui::test]
    fn a_superseded_run_publishes_nothing(cx: &mut TestAppContext) {
        register(cx, &[("flag", "bad")]);
        cx.update(|cx| {
            Validators::run(&grid().0, &grid().1, &[], cx);
            Validators::run(&grid().0, &fixed(), &[], cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(Diagnostics::all(cx).is_empty()));
    }

    #[gpui::test]
    fn a_validator_removed_mid_run_is_not_resurrected(cx: &mut TestAppContext) {
        register(cx, &[("flag", "bad")]);
        cx.update(|cx| {
            Validators::run(&grid().0, &grid().1, &[], cx);
            Validators::remove(&"flag".into(), cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(Diagnostics::all(cx).is_empty()));
    }

    #[gpui::test]
    fn a_run_without_registered_validators_touches_nothing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (columns, rows) = grid();
            Validators::run(&columns, &rows, &[], cx);
            assert!(Diagnostics::all(cx).is_empty());
        });
    }

    /// Counts the columns it is asked about, so a test can see which ones a run checked again.
    struct Counting(std::sync::Arc<AtomicUsize>);

    impl ColumnValidator for Counting {
        fn name(&self) -> SharedString {
            "counting".into()
        }

        fn validate(
            &self,
            column: &ColumnInfo,
            values: crate::ColumnValues<'_>,
        ) -> Vec<super::ColumnFinding> {
            self.0.fetch_add(1, Ordering::SeqCst);
            values
                .iter()
                .filter(|value| value.raw == "bad")
                .map(|value| {
                    let message = SharedString::from(format!("bad in {}", column.name));
                    (value.row, Severity::Error, message).into()
                })
                .collect()
        }
    }

    #[gpui::test]
    fn an_edit_checks_only_the_column_it_touched(cx: &mut TestAppContext) {
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        cx.update(|cx| Validators::register(Box::new(Counting(calls.clone())), cx));
        run(cx, &grid().1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let edited: Vec<Vec<SharedString>> = vec![
            vec!["ok".into(), "ok".into()],
            vec!["bad".into(), "ok".into()],
        ];
        run(cx, &edited);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "only Format changed");
        cx.update(|cx| {
            let all = Diagnostics::all(cx);
            assert_eq!(all.len(), 1, "Title's reused finding is still published");
            assert_eq!(all[0].location.column.as_deref(), Some("Title"));
        });

        run(cx, &edited);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "nothing changed");

        let renamed: [(SharedString, SharedString); 2] = [
            ("c0".into(), "Caption".into()),
            ("c1".into(), "Format".into()),
        ];
        cx.update(|cx| Validators::run(&renamed, &edited, &[], cx));
        cx.run_until_parked();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            5,
            "a rename checks every column"
        );
        cx.update(|cx| {
            assert_eq!(
                Diagnostics::all(cx)[0].location.column.as_deref(),
                Some("Caption")
            );
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
            Validators::run(&columns, &rows, &[], cx);
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
            settings: settings.into(),
            values: ["Aderman, Ray".into()].into(),
            subdelimiter: SharedString::default(),
            row_ids: [1].into(),
            revision: 1,
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

    #[test]
    fn addressed_rows_use_their_snapshot_ids() {
        let found = vec![(0, Severity::Error, SharedString::from("bad value")).into()];
        let addressed = crate::address("test".into(), &snapshot(&[]), found);
        assert_eq!(addressed[0].location.row_id, Some(1));
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
            Validators::run(&columns, &rows, &[], cx);
            assert_eq!(RUNS.load(Ordering::SeqCst), 1, "registered twice, ran once");

            // A different name is a different producer and does get its own run.
            crate::AsyncValidators::register("files", count, cx);
            Validators::run(&columns, &rows, &[], cx);
            assert_eq!(RUNS.load(Ordering::SeqCst), 3);
        });
    }
}
