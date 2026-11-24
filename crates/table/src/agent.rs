//! Live-table adapter for the external-agent contract: the reads, and the one draft-only write.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use ai::agent::{
    Column, Count, DiagnosticSummary, FilterOp, Finding, MAX_RESULT_BYTES, Overview,
    ProjectSummary, Query, QueryResult, QuerySource, Rename, RequestError, Severity, Staged,
    StagedRenames, StagedRewrite, TableRevision,
};
use diagnostics::fixes::{GroupFix, SourceActions};
use diagnostics::{DATASET_MAIN, FixProviders, Location, Source};
use gpui::{
    AnyElement, App, Empty, Global, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::notification::Notification;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use settings::columns::ColumnType;
use settings::history::Origin;

use crate::{QrateTableDelegate, TableStateHandle};

/// Prefix of the diagnostics source an agent's staged drafts are filed under; see [`agent_source`].
pub const AGENT_SOURCE: &str = "agent";

/// The name an agent gave itself, back out of its [`agent_source`].
fn agent_name(source: &str) -> &str {
    source
        .strip_prefix(AGENT_SOURCE)
        .map_or(source, |name| name.trim_start_matches(':'))
}

/// The source for one named agent, so its re-run replaces its own drafts and nobody else's.
pub fn agent_source(agent: &str) -> SharedString {
    format!("{AGENT_SOURCE}:{agent}").into()
}

#[derive(Default)]
pub(crate) struct AgentProgram {
    pub(crate) source: String,
    pub(crate) version: u64,
    pub(crate) hash: String,
}
impl Global for AgentProgram {}
pub(crate) static PROGRAM_RUNS: AtomicU64 = AtomicU64::new(1);

pub(crate) fn program_snapshot(
    revision: TableRevision,
    delegate: &QrateTableDelegate,
    cx: &App,
) -> plugin_host::AgentSnapshot {
    let columns: Vec<SharedString> = delegate
        .row_fields(0)
        .into_iter()
        .map(|(column, _)| column)
        .collect();
    let rows = (0..delegate.row_count())
        .map(|row| {
            delegate
                .row_fields(row)
                .into_iter()
                .map(|(_, value)| value)
                .collect()
        })
        .collect();
    let diagnostics = diagnostics::Diagnostics::all(cx)
        .iter()
        .map(|diagnostic| plugin_host::AgentDiagnostic {
            row: diagnostic.location.row,
            column: diagnostic.location.column.as_ref().map(ToString::to_string),
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            source: diagnostic.source.label().to_string(),
            message: diagnostic.message.to_string(),
        })
        .collect();
    plugin_host::AgentSnapshot {
        revision: revision.0,
        columns,
        rows,
        selected_rows: delegate.selected_source_rows(),
        diagnostics,
        files: (0..delegate.row_count())
            .map(|row| delegate.row_image(row).map(ToOwned::to_owned))
            .collect(),
    }
}

pub(crate) fn overview(
    project: &settings::project::CurrentProject,
    delegate: &QrateTableDelegate,
    cx: &App,
) -> Overview {
    let mut summary = DiagnosticSummary::default();
    let mut sources = BTreeMap::<String, usize>::new();
    for diagnostic in diagnostics::Diagnostics::all(cx).iter() {
        match diagnostic.severity {
            diagnostics::Severity::Error => summary.errors += 1,
            diagnostics::Severity::Warning => summary.warnings += 1,
            diagnostics::Severity::Note => summary.notes += 1,
        }
        *sources
            .entry(diagnostic.source.label().to_string())
            .or_default() += 1;
    }
    summary.sources = sources
        .into_iter()
        .map(|(value, count)| Count { value, count })
        .collect();
    Overview {
        project: ProjectSummary {
            name: project.display_name(),
            row_count: delegate.row_count(),
            column_count: delegate.column_count(),
            has_files_folder: project
                .data
                .values
                .get(settings::project::FILES_FOLDER_KEY)
                .is_some_and(|value| !value.text().trim().is_empty()),
        },
        columns: project
            .data
            .headers
            .iter()
            .map(|name| {
                let configured = project
                    .data
                    .columns
                    .iter()
                    .find(|column| column.name == *name);
                Column {
                    name: name.clone(),
                    data_type: configured
                        .map_or_else(String::new, |column| column.data_type.clone()),
                    notes: configured.map_or_else(String::new, |column| column.notes.clone()),
                }
            })
            .collect(),
        selected_rows: delegate.selected_source_rows().len(),
        diagnostics: summary,
    }
}

type Record = HashMap<String, serde_json::Value>;

pub(crate) fn run_query_records(
    query: Query,
    revision: TableRevision,
    mut records: Vec<Record>,
    source_schema: Vec<String>,
    diagnostic_source: bool,
) -> Result<QueryResult, RequestError> {
    let fingerprint = query_fingerprint(&query);
    let offset = cursor_offset(query.cursor.as_deref(), revision, fingerprint)?;
    if let QuerySource::Search { text: needle } = &query.source {
        let needle = needle.to_lowercase();
        records.retain(|record| {
            record.iter().any(|(field, value)| {
                field.to_lowercase().contains(&needle)
                    || text(Some(value)).to_lowercase().contains(&needle)
            })
        });
    }
    records.retain(|record| {
        query
            .filters
            .iter()
            .all(|filter| filter_matches(record, filter))
    });

    let referenced = query
        .select
        .iter()
        .chain(query.group_by.iter())
        .chain(query.distinct.iter())
        .chain(query.filters.iter().map(|filter| &filter.field))
        .chain(query.order_by.iter().map(|order| &order.field));
    if referenced
        .into_iter()
        .any(|field| !source_schema.contains(field))
    {
        return Err(RequestError::UnknownField);
    }

    let mut fields = if let Some(distinct) = &query.distinct {
        let mut values = records
            .iter()
            .map(|record| text(record.get(distinct)))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        records = values
            .into_iter()
            .map(|value| Record::from([(distinct.clone(), value.into())]))
            .collect();
        vec![distinct.clone()]
    } else if query.group_by.is_empty() {
        if query.select.is_empty() {
            if diagnostic_source {
                ["row", "column", "severity", "source", "message"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            } else {
                source_schema.clone()
            }
        } else {
            query.select.clone()
        }
    } else {
        let mut fields = query.group_by.clone();
        fields.push("count".into());
        let mut grouped = BTreeMap::<Vec<String>, usize>::new();
        for record in &records {
            let key = query
                .group_by
                .iter()
                .map(|field| text(record.get(field)))
                .collect();
            *grouped.entry(key).or_default() += 1;
        }
        records = grouped
            .into_iter()
            .map(|(key, count)| {
                let mut record = Record::new();
                for (field, value) in query.group_by.iter().zip(key) {
                    record.insert(field.clone(), value.into());
                }
                record.insert("count".into(), count.into());
                record
            })
            .collect();
        fields
    };
    if fields.is_empty() {
        fields.push("row".into());
    }
    if let Some(order) = &query.order_by {
        if !fields.contains(&order.field)
            && (query.distinct.is_some() || !query.group_by.is_empty())
        {
            return Err(RequestError::UnknownField);
        }
        records.sort_by_key(|record| text(record.get(&order.field)).to_lowercase());
        if order.descending {
            records.reverse();
        }
    }

    let total = records.len();
    if offset > total {
        return Err(RequestError::InvalidCursor);
    }
    let mut items = Vec::new();
    // Reserve the schema and bounded pagination metadata, then serialize each candidate once.
    let mut used = serde_json::to_vec(&fields).map_or(MAX_RESULT_BYTES, |value| value.len()) + 512;
    let end = total.min(offset + query.limit);
    for record in &records[offset..end] {
        let item: Vec<_> = fields
            .iter()
            .map(|field| {
                record
                    .get(field)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            })
            .collect();
        let prospective = serde_json::to_vec(&item).map_or(MAX_RESULT_BYTES + 1, |v| v.len());
        if used + prospective > MAX_RESULT_BYTES && items.is_empty() {
            return Err(RequestError::ResultItemTooLarge);
        }
        if used + prospective > MAX_RESULT_BYTES {
            break;
        }
        used += prospective;
        items.push(item);
    }
    let consumed = offset + items.len();
    let remaining = total.saturating_sub(consumed);
    let next_cursor =
        (remaining > 0).then(|| format!("{}:{fingerprint:016x}:{consumed}", revision.0));
    Ok(QueryResult {
        fields,
        returned: items.len(),
        items,
        remaining,
        truncated: remaining > 0,
        next_cursor,
    })
}

pub(crate) fn row_records(delegate: &QrateTableDelegate, source: &QuerySource) -> Vec<Record> {
    let rows: Vec<usize> = match source {
        QuerySource::AllRows | QuerySource::Search { .. } => (0..delegate.row_count()).collect(),
        QuerySource::SelectedRows => delegate.selected_source_rows(),
        QuerySource::Rows { rows } => rows.clone(),
        QuerySource::Diagnostics { .. } => unreachable!(),
    };
    rows.into_iter()
        .filter(|row| *row < delegate.row_count())
        .map(|row| {
            let mut record = Record::from([("row".into(), row.into())]);
            for (column, value) in delegate.row_fields(row) {
                record.insert(column.to_string(), value.to_string().into());
            }
            record
        })
        .collect()
}

pub(crate) fn diagnostic_records(
    cx: &App,
    severities: &[Severity],
    sources: &[String],
) -> Vec<Record> {
    diagnostics::Diagnostics::all(cx)
        .iter()
        .filter_map(|diagnostic| {
            let severity = match diagnostic.severity {
                diagnostics::Severity::Error => Severity::Error,
                diagnostics::Severity::Warning => Severity::Warning,
                diagnostics::Severity::Note => Severity::Note,
            };
            let source = diagnostic.source.label().to_string();
            if (!severities.is_empty() && !severities.contains(&severity))
                || (!sources.is_empty() && !sources.contains(&source))
            {
                return None;
            }
            Some(Record::from([
                (
                    "row".into(),
                    diagnostic
                        .location
                        .row
                        .map_or(serde_json::Value::Null, Into::into),
                ),
                (
                    "column".into(),
                    diagnostic
                        .location
                        .column
                        .as_ref()
                        .map_or(serde_json::Value::Null, |v| v.to_string().into()),
                ),
                (
                    "severity".into(),
                    format!("{severity:?}").to_lowercase().into(),
                ),
                ("source".into(), source.into()),
                ("message".into(), diagnostic.message.to_string().into()),
            ]))
        })
        .collect()
}

fn filter_matches(record: &Record, filter: &ai::agent::QueryFilter) -> bool {
    let actual = text(record.get(&filter.field));
    let expected = filter.value.as_deref().unwrap_or_default();
    match filter.op {
        FilterOp::Equals => actual.eq_ignore_ascii_case(expected),
        FilterOp::NotEquals => !actual.eq_ignore_ascii_case(expected),
        FilterOp::Contains => actual.to_lowercase().contains(&expected.to_lowercase()),
        FilterOp::StartsWith => actual.to_lowercase().starts_with(&expected.to_lowercase()),
        FilterOp::EndsWith => actual.to_lowercase().ends_with(&expected.to_lowercase()),
        FilterOp::IsBlank => actual.is_empty(),
        FilterOp::IsNotBlank => !actual.is_empty(),
    }
}

fn text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn query_fingerprint(query: &Query) -> u64 {
    let mut normalized = query.clone();
    normalized.cursor = None;
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(&normalized)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

fn cursor_offset(
    cursor: Option<&str>,
    revision: TableRevision,
    fingerprint: u64,
) -> Result<usize, RequestError> {
    let Some(cursor) = cursor else { return Ok(0) };
    let mut parts = cursor.split(':');
    let cursor_revision = parts
        .next()
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or(RequestError::InvalidCursor)?;
    let cursor_fingerprint = parts
        .next()
        .and_then(|v| u64::from_str_radix(v, 16).ok())
        .ok_or(RequestError::InvalidCursor)?;
    let offset = parts
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or(RequestError::InvalidCursor)?;
    if parts.next().is_some() || cursor_fingerprint != fingerprint {
        return Err(RequestError::InvalidCursor);
    }
    if cursor_revision != revision.0 {
        return Err(RequestError::StaleCursor);
    }
    Ok(offset)
}

/// One agent proposal, held until somebody opens the Fixes menu on its cell.
struct StagedFix {
    /// The cell text the agent judged. The offer is withheld unless the cell still says this.
    expected: SharedString,
    replacement: SharedString,
}

/// One column's staged new values: source row → (text when staged, new text).
struct Rewrite {
    note: Option<String>,
    rows: BTreeMap<usize, (SharedString, SharedString)>,
    /// The column holds file links: applying repoints each row's file, not just its text.
    relink: bool,
}

impl Rewrite {
    fn title(&self, column: &str) -> String {
        let note = self.note.as_ref().map(|note| format!(" ({note})"));
        let verb = match self.relink {
            true => "Relink",
            false => "Rewrite",
        };
        format!(
            "{verb} {column}: {} rows{}",
            self.rows.len(),
            note.unwrap_or_default()
        )
    }
}

/// Write `cells`, as file links when `relink`, as one undo step.
fn write(cells: Vec<(usize, usize, SharedString)>, relink: bool, origin: Origin, cx: &mut App) {
    match relink {
        true => crate::write_links(cells, origin, cx),
        false => crate::write_cells(cells, origin, cx),
    }
}

/// Everything one agent has staged: its findings, their replacements by `(row, column)`, and its
/// column rewrites by column.
#[derive(Default)]
struct Batch {
    findings: Vec<diagnostics::Diagnostic>,
    by_cell: HashMap<(usize, SharedString), Vec<StagedFix>>,
    rewrites: BTreeMap<SharedString, Rewrite>,
    renames: Vec<StagedRename>,
    /// Rows a change set adds, as column name → value.
    adds: Vec<BTreeMap<String, String>>,
    /// Rows a change set deletes, and the agent's reason.
    deletes: Option<(Vec<settings::project::RowId>, String)>,
    /// Staged as one change set: applying it is one undo step.
    atomic: bool,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.findings.is_empty()
            && self.rewrites.is_empty()
            && self.renames.is_empty()
            && self.adds.is_empty()
            && self.deletes.is_none()
    }
}

/// One staged file rename, by row identity so rows added or removed since do not redirect it.
struct StagedRename {
    row: settings::project::RowId,
    /// The column whose cell names the file, when one does.
    column: Option<SharedString>,
    from: PathBuf,
    to: PathBuf,
}

fn renames_title(count: usize) -> String {
    format!("Rename {count} files")
}

/// Every agent's batch, by its diagnostics source.
///
/// A global rather than a field on anything, because [`FixProviders`] registers a plain `fn`
/// pointer and the offer has no handle to reach back through. Never persisted, and an agent's
/// batch is replaced wholesale by its next one — the replace-by-source rule diagnostics follow.
#[derive(Default)]
struct StagedFixes(HashMap<SharedString, Batch>);
impl Global for StagedFixes {}

/// Publish a batch of drafts: findings into the Problems panel, replacements into the Fixes menu.
/// No cell is touched, and a draft the table has moved past is dropped rather than shown.
pub(crate) fn stage_findings(
    agent: &str,
    revision: TableRevision,
    findings: Vec<Finding>,
    cx: &mut App,
) -> Result<Staged, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;

    let source = agent_source(agent);
    let mut published = Vec::new();
    let mut fixes: HashMap<(usize, SharedString), Vec<StagedFix>> = HashMap::new();
    let mut stale = Vec::new();

    let table = state.read(cx);
    let delegate = table.delegate();
    let current = TableRevision(delegate.values_generation());

    for finding in findings
        .iter()
        .filter(|finding| finding.replacement.is_some())
    {
        if let Some(col) = delegate.data_col(&finding.column) {
            not_a_file_column(delegate, col)?;
        }
    }
    for (ix, finding) in findings.into_iter().enumerate() {
        let cell = (finding.row < delegate.row_count())
            .then(|| delegate.row_fields(finding.row))
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|(column, _)| *column == finding.column)
                    .map(|(_, value)| value)
            });
        // A row past the end, a column that no longer exists, or a cell edited since the review:
        // publishing any of those would point the archivist at text the agent never read.
        let Some(cell) = cell.filter(|cell| *cell == finding.expected) else {
            stale.push(ix);
            continue;
        };

        let column = SharedString::from(finding.column);
        published.push(diagnostics::Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: Some(finding.row),
                row_id: delegate.row_id(finding.row),
                column: Some(column.clone()),
            },
            severity: match finding.severity {
                Severity::Error => diagnostics::Severity::Error,
                Severity::Warning => diagnostics::Severity::Warning,
                Severity::Note => diagnostics::Severity::Note,
            },
            source: Source::Validator(source.clone()),
            message: finding.message.into(),
            group: None,
            // A computed finding carries no filing stamp — it is recomputed, not observed once.
            filed: None,
        });

        if let Some(replacement) = finding.replacement {
            fixes
                .entry((finding.row, column))
                .or_default()
                .push(StagedFix {
                    expected: cell,
                    replacement: replacement.into(),
                });
        }
    }

    let accepted = published.len();
    let batch = cx
        .default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default();
    batch.findings = published;
    batch.by_cell = fixes;
    publish(&source, cx);
    log::info!(
        "{agent} staged {accepted} finding(s) against revision {}, dropping {} judged against \
         text the table has moved past (current revision {})",
        revision.0,
        stale.len(),
        current.0
    );

    Ok(Staged { accepted, stale })
}

/// What the Fixes menu offers for a staged finding — but only while the cell still says what the
/// agent judged, because a cell edited since staging is a cell nobody reviewed.
fn offer_staged(
    location: &Location,
    text: &str,
    _: Option<&str>,
    cx: &App,
) -> Vec<diagnostics::Fix> {
    let (Some(row), Some(column)) = (location.row, location.column.clone()) else {
        return Vec::new();
    };
    let cell = (row, column);
    cx.try_global::<StagedFixes>()
        .into_iter()
        .flat_map(|staged| &staged.0)
        .filter_map(|(source, batch)| Some((agent_name(source), batch.by_cell.get(&cell)?)))
        .flat_map(|(agent, fixes)| {
            fixes
                .iter()
                .filter(|fix| fix.expected == text)
                .map(move |fix| diagnostics::Fix {
                    label: format!("Use “{}”", fix.replacement).into(),
                    replacement: fix.replacement.clone(),
                    agent: Some(agent.to_owned().into()),
                })
        })
        .collect()
}

/// Whole-batch actions on any of one agent's findings: apply every staged fix, or drop the batch.
fn staged_actions(source: &str, cx: &App) -> Vec<GroupFix> {
    let Some(batch) = cx
        .try_global::<StagedFixes>()
        .and_then(|staged| staged.0.get(source))
    else {
        return Vec::new();
    };
    let agent = agent_name(source);
    let fixes = batch.by_cell.len();
    let source = SharedString::from(source.to_owned());
    let mut actions = Vec::new();
    if fixes > 0 {
        let source = source.clone();
        actions.push(GroupFix::action(
            format!("Apply all {fixes} fixes from {agent}"),
            move |cx| {
                let (applied, stale) = apply_staged(&source, cx);
                tell(format!("{applied} applied, {stale} skipped as stale"), cx);
            },
        ));
    }
    if !batch.findings.is_empty() {
        let source = source.clone();
        actions.push(GroupFix::action("Discard all", move |cx| {
            discard_staged(&source, cx)
        }));
    }
    for (column, rewrite) in &batch.rewrites {
        let title = rewrite.title(column);
        let at = (source.clone(), column.clone());
        actions.push(GroupFix::action(format!("Apply: {title}"), {
            let at = at.clone();
            move |cx| {
                let (applied, stale) = apply_rewrite(&at.0, &at.1, cx);
                tell(format!("{applied} applied, {stale} skipped as stale"), cx);
            }
        }));
        actions.push(GroupFix::action(format!("Discard: {title}"), move |cx| {
            discard_rewrite(&at.0, &at.1, cx)
        }));
    }
    if !batch.renames.is_empty() {
        let title = renames_title(batch.renames.len());
        actions.push(GroupFix::action(format!("Apply: {title}"), {
            let source = source.clone();
            move |cx| {
                let message = match apply_renames(&source, cx) {
                    Ok((applied, stale)) => format!("{applied} renamed, {stale} skipped as stale"),
                    Err(why) => format!("No file was renamed: {why}"),
                };
                tell(message, cx);
            }
        }));
        actions.push(GroupFix::action(format!("Discard: {title}"), move |cx| {
            if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(&source) {
                batch.renames.clear();
            }
            publish(&source, cx);
        }));
    }
    actions
}

/// Write every staged fix whose cell still reads what the agent judged, as one undo step, and
/// retire the batch. Returns how many were written and how many were skipped as stale.
fn apply_staged(source: &SharedString, cx: &mut App) -> (usize, usize) {
    let Some(batch) = cx
        .try_global::<StagedFixes>()
        .and_then(|staged| staged.0.get(source))
    else {
        return (0, 0);
    };
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return (0, 0);
    };
    let delegate = state.read(cx).delegate();
    let mut cells: Vec<_> = batch
        .by_cell
        .iter()
        .filter_map(|((row, column), fixes)| {
            let col = delegate.data_col(column)?;
            let text = delegate.cell(*row, col)?;
            let fix = fixes.iter().find(|fix| fix.expected == *text)?;
            Some((*row, col, fix.replacement.clone()))
        })
        .collect();
    cells.sort_unstable_by_key(|(row, col, _)| (*row, *col));
    let (applied, stale) = (cells.len(), batch.by_cell.len() - cells.len());
    let agent = agent_name(source).to_owned();
    discard_staged(source, cx);
    let origin = Origin::Agent {
        agent: agent.clone(),
        fix: format!("Apply {applied} staged fixes"),
    };
    crate::write_cells(cells, origin, cx);
    log::info!("{agent}: {applied} staged fix(es) applied, {stale} skipped as stale");
    (applied, stale)
}

/// Drop one agent's findings and their fixes without touching a cell.
fn discard_staged(source: &SharedString, cx: &mut App) {
    if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) {
        batch.findings.clear();
        batch.by_cell.clear();
    }
    publish(source, cx);
}

/// Show what one agent has staged: its findings, and one entry per column rewrite.
fn publish(source: &SharedString, cx: &mut App) {
    let staged = cx.default_global::<StagedFixes>();
    if staged.0.get(source).is_some_and(Batch::is_empty) {
        staged.0.remove(source);
    }
    let items = staged.0.get(source).map_or_else(Vec::new, |batch| {
        let rewrites = batch
            .rewrites
            .iter()
            .map(|(column, rewrite)| diagnostics::Diagnostic {
                location: Location {
                    dataset: DATASET_MAIN.into(),
                    row: None,
                    row_id: None,
                    column: Some(column.clone()),
                },
                severity: diagnostics::Severity::Note,
                source: Source::Validator(source.clone()),
                message: rewrite.title(column).into(),
                group: None,
                filed: None,
            });
        let renames = (!batch.renames.is_empty()).then(|| diagnostics::Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: None,
                row_id: None,
                column: None,
            },
            severity: diagnostics::Severity::Note,
            source: Source::Validator(source.clone()),
            message: renames_title(batch.renames.len()).into(),
            group: None,
            filed: None,
        });
        let whole = |message: String| diagnostics::Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: None,
                row_id: None,
                column: None,
            },
            severity: diagnostics::Severity::Note,
            source: Source::Validator(source.clone()),
            message: message.into(),
            group: None,
            filed: None,
        };
        let adds =
            (!batch.adds.is_empty()).then(|| whole(format!("Add {} rows", batch.adds.len())));
        let deletes = batch
            .deletes
            .as_ref()
            .map(|(rows, reason)| whole(format!("Delete {} rows: {reason}", rows.len())));
        batch
            .findings
            .iter()
            .cloned()
            .chain(rewrites)
            .chain(renames)
            .chain(adds)
            .chain(deletes)
            .collect()
    });
    diagnostics::Diagnostics::set(&Source::Validator(source.clone()), DATASET_MAIN, items, cx);
    FixProviders::register(source, offer_staged, cx);
    SourceActions::register(source, staged_actions, cx);
    // The grid marks staged cells and the views show the staged bar; both have to redraw.
    if let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    {
        state.update(cx, |_, cx| cx.notify());
    }
    cx.refresh_windows();
}

/// Refuse a draft that would retype a file column's text: the row would then name a file that does
/// not exist, and nothing on disk would move.
fn not_a_file_column(delegate: &QrateTableDelegate, col: usize) -> Result<(), RequestError> {
    match delegate.column_type(col) == ColumnType::Filename {
        true => Err(RequestError::FileColumn(
            delegate.column_name(col).to_string(),
        )),
        false => Ok(()),
    }
}

/// Stage one column's new values as a single draft, replacing this agent's earlier rewrite of the
/// same column. An empty `values` retracts it. No cell is touched.
pub(crate) fn stage_column_rewrite(
    agent: &str,
    column: String,
    values: Vec<(usize, String)>,
    note: Option<String>,
    cx: &mut App,
) -> Result<StagedRewrite, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let current = TableRevision(delegate.values_generation());
    let col = delegate
        .data_col(&column)
        .ok_or(RequestError::UnknownColumn)?;
    not_a_file_column(delegate, col)?;
    let mut rows = BTreeMap::new();
    let mut unchanged = Vec::new();
    for (row, value) in values {
        let before = delegate
            .cell(row, col)
            .ok_or(RequestError::RewriteRowOutOfRange)?;
        if *before == value {
            unchanged.push(row);
        } else {
            rows.insert(row, (before.clone(), SharedString::from(value)));
        }
    }

    let accepted = rows.len();
    let source = agent_source(agent);
    let batch = cx
        .default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default();
    if rows.is_empty() {
        batch.rewrites.remove(column.as_str());
    } else {
        batch.rewrites.insert(
            column.clone().into(),
            Rewrite {
                note,
                rows,
                relink: false,
            },
        );
    }
    publish(&source, cx);
    log::info!(
        "{agent} staged a rewrite of {accepted} cell(s) in {column} at revision {}, {} already \
         holding the value",
        current.0,
        unchanged.len()
    );

    Ok(StagedRewrite {
        accepted,
        unchanged,
    })
}

/// Stage several columns of several rows: added to this agent's draft of each column touched, a
/// row staged again taking the newer value. A row whose `expect` no longer matches is
/// skipped as stale.
pub(crate) fn stage_patch(
    agent: &str,
    patch: Vec<ai::agent::PatchRow>,
    note: Option<String>,
    cx: &mut App,
) -> Result<ai::agent::StagedPatch, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let mut columns: BTreeMap<SharedString, BTreeMap<usize, (SharedString, SharedString)>> =
        BTreeMap::new();
    let (mut unchanged, mut stale) = (0, Vec::new());
    for row in patch {
        let cell = |column: &str| {
            let col = delegate
                .data_col(column)
                .ok_or(RequestError::UnknownColumn)?;
            delegate
                .cell(row.row, col)
                .cloned()
                .ok_or(RequestError::RewriteRowOutOfRange)
        };
        let mut matches = true;
        for (column, expected) in &row.expect {
            matches &= cell(column)? == expected.as_str();
        }
        if !matches {
            stale.push(row.row);
            continue;
        }
        for (column, value) in row.set {
            not_a_file_column(
                delegate,
                delegate
                    .data_col(&column)
                    .ok_or(RequestError::UnknownColumn)?,
            )?;
            let before = cell(&column)?;
            if before == value.as_str() {
                unchanged += 1;
                continue;
            }
            columns
                .entry(column.into())
                .or_default()
                .insert(row.row, (before, value.into()));
        }
    }
    let accepted = columns.values().map(BTreeMap::len).sum();
    let touched: Vec<String> = columns.keys().map(ToString::to_string).collect();
    let source = agent_source(agent);
    let batch = cx
        .default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default();
    for (column, rows) in columns {
        let draft = batch.rewrites.entry(column).or_insert_with(|| Rewrite {
            note: None,
            rows: BTreeMap::new(),
            relink: false,
        });
        draft.note = note.clone().or(draft.note.take());
        draft.rows.extend(rows);
    }
    publish(&source, cx);
    log::info!(
        "{agent} staged a patch of {accepted} cell(s) in {} column(s), {unchanged} unchanged, {} \
         row(s) stale",
        touched.len(),
        stale.len()
    );
    Ok(ai::agent::StagedPatch {
        accepted,
        unchanged,
        stale,
        columns: touched,
    })
}

/// Stage file links as `(row, column, file)`: the cell will hold the link text qrate writes for
/// that file, and applying repoints the row as relinking does. `column` defaults to the row's first
/// file column.
pub(crate) fn stage_relinks(
    agent: &str,
    links: Vec<(usize, Option<String>, PathBuf)>,
    note: Option<String>,
    cx: &mut App,
) -> Result<ai::agent::StagedRelinks, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let folder = cx
        .try_global::<settings::project::CurrentProject>()
        .and_then(|project| project.data.values.get(settings::project::FILES_FOLDER_KEY))
        .map(|value| PathBuf::from(value.text().as_ref()))
        .filter(|folder| !folder.as_os_str().is_empty());
    let delegate = state.read(cx).delegate();
    let file_column =
        (0..delegate.column_count()).find(|col| delegate.column_type(*col) == ColumnType::Filename);
    let mut columns: BTreeMap<SharedString, BTreeMap<usize, (SharedString, SharedString)>> =
        BTreeMap::new();
    let mut unchanged = Vec::new();
    for (row, column, file) in links {
        if !file.is_file() {
            return Err(RequestError::NotAFile(file.display().to_string()));
        }
        let col = match &column {
            Some(name) => delegate.data_col(name).ok_or(RequestError::UnknownColumn)?,
            None => file_column.ok_or(RequestError::UnknownColumn)?,
        };
        let before = delegate
            .cell(row, col)
            .cloned()
            .ok_or(RequestError::RowOutOfRange)?;
        // `file` is canonical, so the files folder must be too before one is made relative to the other.
        let file = dunce::simplified(&file).to_path_buf();
        let after: SharedString = match &folder {
            Some(folder) => {
                let folder = folder
                    .canonicalize()
                    .map(|folder| dunce::simplified(&folder).to_path_buf())
                    .unwrap_or_else(|_| folder.clone());
                crate::relink::link_text(&folder, &file)
            }
            None => file_ingest::normalized_path(&file),
        }
        .into();
        if before == after {
            unchanged.push(row);
            continue;
        }
        columns
            .entry(delegate.column_name(col).to_string().into())
            .or_default()
            .insert(row, (before, after));
    }
    let accepted = columns.values().map(BTreeMap::len).sum();
    let source = agent_source(agent);
    let batch = cx
        .default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default();
    for (column, rows) in columns {
        let draft = batch.rewrites.entry(column).or_insert_with(|| Rewrite {
            note: None,
            rows: BTreeMap::new(),
            relink: true,
        });
        draft.note = note.clone().or(draft.note.take());
        draft.rows.extend(rows);
    }
    publish(&source, cx);
    log::info!("{agent} staged {accepted} relink(s)");
    Ok(ai::agent::StagedRelinks {
        accepted,
        unchanged,
    })
}

/// Write a staged rewrite's rows that still read what they read when staged, as one undo step, and
/// retire it. Returns how many were written and how many were skipped as stale.
fn apply_rewrite(source: &SharedString, column: &SharedString, cx: &mut App) -> (usize, usize) {
    let Some(rewrite) = cx
        .default_global::<StagedFixes>()
        .0
        .get_mut(source)
        .and_then(|batch| batch.rewrites.remove(column))
    else {
        return (0, 0);
    };
    publish(source, cx);
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return (0, 0);
    };
    let delegate = state.read(cx).delegate();
    let cells: Vec<_> = delegate
        .data_col(column)
        .into_iter()
        .flat_map(|col| {
            rewrite
                .rows
                .iter()
                .filter(move |(row, (before, _))| delegate.cell(**row, col) == Some(before))
                .map(move |(row, (_, after))| (*row, col, after.clone()))
        })
        .collect();
    let (applied, stale) = (cells.len(), rewrite.rows.len() - cells.len());
    let agent = agent_name(source).to_owned();
    let origin = Origin::Agent {
        agent: agent.clone(),
        fix: rewrite.title(column),
    };
    write(cells, rewrite.relink, origin, cx);
    log::info!(
        "{agent}: rewrite of {column} applied to {applied} cell(s), {stale} skipped as stale"
    );
    (applied, stale)
}

/// Drop a staged rewrite without touching a cell.
fn discard_rewrite(source: &SharedString, column: &SharedString, cx: &mut App) {
    if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) {
        batch.rewrites.remove(column);
    }
    publish(source, cx);
}

/// The folder holding the project file, which every staged path is relative to.
fn project_dir(cx: &App) -> Result<PathBuf, RequestError> {
    cx.try_global::<settings::project::CurrentProject>()
        .and_then(|project| project.file.parent().map(Path::to_path_buf))
        .ok_or(RequestError::ProjectUnavailable)
}

/// Whether `cell` is a name the files folder would resolve to a file called `name`.
fn names(cell: &str, name: &Path) -> bool {
    let keys = settings::filenames::keys(&name.file_name().unwrap_or_default().to_string_lossy());
    settings::filenames::lookup_keys(cell)
        .iter()
        .any(|key| keys.contains(key))
}

/// Stage renames of files the rows link to, replacing this agent's earlier renames. An empty batch
/// retracts them. Every rename must be sound or the whole batch is refused, and no file is touched.
pub(crate) fn stage_renames(
    agent: &str,
    renames: Vec<Rename>,
    cx: &mut App,
) -> Result<StagedRenames, RequestError> {
    let root = project_dir(cx)?;
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let current = TableRevision(delegate.values_generation());

    let mut staged = Vec::with_capacity(renames.len());
    for rename in renames {
        let inside = |path: &str| {
            file_ingest::rename::inside(&root, path)
                .ok_or_else(|| RequestError::RenameOutsideProject(path.to_owned()))
        };
        let (from, to) = (inside(&rename.from)?, inside(&rename.to)?);
        let linked = delegate
            .row_image(rename.row)
            .and_then(|image| image.canonicalize().ok());
        let (Some(row), true) = (
            delegate.row_id(rename.row),
            from.is_file() && linked.is_some() && linked == from.canonicalize().ok(),
        ) else {
            return Err(RequestError::RenameSourceMismatch(rename.from));
        };
        if from == to {
            continue;
        }
        let fields = delegate.row_fields(rename.row);
        let column = (0..fields.len())
            .filter(|col| names(&fields[*col].1, &from))
            .max_by_key(|col| delegate.column_type(*col) == ColumnType::Filename)
            .map(|col| fields[col].0.clone());
        staged.push(StagedRename {
            row,
            column,
            from,
            to,
        });
    }
    let sources: HashSet<&Path> = staged.iter().map(|rename| rename.from.as_path()).collect();
    let mut targets = HashSet::new();
    for rename in &staged {
        let display =
            || file_ingest::normalized_path(rename.to.strip_prefix(&root).unwrap_or(&rename.to));
        if !targets.insert(rename.to.as_path()) || sources.len() != staged.len() {
            return Err(RequestError::RenameTargetCollision(display()));
        }
        if rename.to.exists() && !sources.contains(rename.to.as_path()) {
            return Err(RequestError::RenameTargetExists(display()));
        }
    }

    let accepted = staged.len();
    let source = agent_source(agent);
    cx.default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default()
        .renames = staged;
    publish(&source, cx);
    log::info!(
        "{agent} staged {accepted} file rename(s) at revision {}",
        current.0
    );
    Ok(StagedRenames { accepted })
}

/// Rename the staged files on disk — all of them or none — then make the rows follow, as one undo
/// step. A rename whose file is gone or whose row links another file by now is skipped as stale.
/// Returns how many were renamed and how many were skipped.
fn apply_renames(source: &SharedString, cx: &mut App) -> Result<(usize, usize), String> {
    apply_some_renames(source, None, cx)
}

/// What a cell naming `from` should say once the file is `to`, or `None` when it already does.
/// A cell that held the bare stem keeps holding one; anything else gets the file name.
fn renamed_text(text: &str, from: &Path, to: &Path) -> Option<String> {
    if names(text, to) {
        return None;
    }
    let stem = |path: &Path| path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let name = |path: &Path| path.file_name().map(|s| s.to_string_lossy().into_owned());
    match stem(from).is_some_and(|was| was.eq_ignore_ascii_case(text.trim())) {
        true => stem(to),
        false => name(to),
    }
}

/// Rename the staged files, or only the one for `only`'s row; the rest stay staged.
fn apply_some_renames(
    source: &SharedString,
    only: Option<settings::project::RowId>,
    cx: &mut App,
) -> Result<(usize, usize), String> {
    let staged: Vec<StagedRename> = cx
        .default_global::<StagedFixes>()
        .0
        .get_mut(source)
        .map(|batch| {
            let (taken, kept) = std::mem::take(&mut batch.renames)
                .into_iter()
                .partition(|rename| only.is_none_or(|row| rename.row == row));
            batch.renames = kept;
            taken
        })
        .unwrap_or_default();
    publish(source, cx);
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or("no project is open")?;
    let delegate = state.read(cx).delegate();

    // A row whose name cell was rewritten first resolves to no file yet; that is not stale.
    let live: Vec<(usize, &StagedRename)> = staged
        .iter()
        .filter_map(|rename| Some((delegate.row_of(rename.row)?, rename)))
        .filter(|(row, rename)| {
            rename.from.is_file()
                && delegate.row_image(*row).is_none_or(|image| {
                    image.canonicalize().ok() == rename.from.canonicalize().ok()
                })
        })
        .collect();
    let stale = staged.len() - live.len();
    let sources: HashSet<&Path> = live.iter().map(|(_, r)| r.from.as_path()).collect();
    if let Some((_, taken)) = live
        .iter()
        .find(|(_, r)| r.to.exists() && !sources.contains(r.to.as_path()))
    {
        return Err(format!("{} already exists", taken.to.display()));
    }

    let files: Vec<(PathBuf, PathBuf)> = live
        .iter()
        .map(|(_, rename)| (rename.from.clone(), rename.to.clone()))
        .collect();
    let cells: Vec<(usize, usize, SharedString)> = live
        .iter()
        .filter_map(|(row, rename)| {
            let col = delegate.data_col(rename.column.as_ref()?)?;
            let after = renamed_text(delegate.cell(*row, col)?, &rename.from, &rename.to)?;
            Some((*row, col, after.into()))
        })
        .collect();
    let rows: Vec<(usize, PathBuf)> = live
        .iter()
        .map(|(row, rename)| (*row, rename.to.clone()))
        .collect();

    for (_, to) in &files {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "the folder {} could not be created: {error}",
                    parent.display()
                )
            })?;
        }
    }
    file_ingest::rename::apply(&files, &mut |from, to| std::fs::rename(from, to)).map_err(
        |failed| {
            log::error!("staged renames from {source} were not applied: {failed}");
            failed.to_string()
        },
    )?;
    let applied = files.len();
    let agent = agent_name(source).to_owned();
    let origin = Origin::Agent {
        agent: agent.clone(),
        fix: match applied {
            1 => "Rename a file".to_owned(),
            n => format!("Rename {n} files"),
        },
    };
    crate::write_renames(cells, rows, files, origin, cx);
    log::info!("{agent}: {applied} file(s) renamed, {stale} skipped as stale");
    Ok((applied, stale))
}

/// Run `show` in the active window after the current update, since a menu click is one already.
pub(crate) fn in_active_window(cx: &mut App, show: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| {
        let shown = cx
            .active_window()
            .map(|window| window.update(cx, |_, window, cx| show(window, cx)));
        if !matches!(shown, Some(Ok(()))) {
            log::warn!(
                "an agent draft's preview or result could not be shown: no window is active"
            );
        }
    });
}

/// A staged value the grid shows on its cell until it is applied or rejected.
pub(crate) struct CellChange {
    pub(crate) before: SharedString,
    pub(crate) after: SharedString,
    pub(crate) agent: SharedString,
}

enum StagedKind {
    Fix,
    Rewrite,
    Rename(settings::project::RowId),
}

fn find_change(
    delegate: &QrateTableDelegate,
    row: usize,
    col: usize,
    cx: &App,
) -> Option<(SharedString, StagedKind, CellChange)> {
    let staged = cx
        .try_global::<StagedFixes>()
        .filter(|staged| !staged.0.is_empty())?;
    let text = delegate.cell(row, col)?.clone();
    let column = delegate.column_name(col);
    let row_id = delegate.row_id(row);
    staged.0.iter().find_map(|(source, batch)| {
        let change = |after: SharedString| CellChange {
            before: text.clone(),
            after,
            agent: agent_name(source).to_owned().into(),
        };
        if let Some(fix) = batch
            .by_cell
            .get(&(row, column.clone()))
            .and_then(|fixes| fixes.iter().find(|fix| fix.expected == text))
        {
            return Some((
                source.clone(),
                StagedKind::Fix,
                change(fix.replacement.clone()),
            ));
        }
        if let Some((before, after)) = batch.rewrites.get(&column).and_then(|r| r.rows.get(&row))
            && *before == text
        {
            return Some((source.clone(), StagedKind::Rewrite, change(after.clone())));
        }
        batch
            .renames
            .iter()
            .find(|rename| Some(rename.row) == row_id && rename.column.as_ref() == Some(&column))
            .and_then(|rename| {
                let after = renamed_text(&text, &rename.from, &rename.to)?;
                Some((
                    source.clone(),
                    StagedKind::Rename(rename.row),
                    change(after.into()),
                ))
            })
    })
}

/// The change an agent staged on this cell, if one still applies to what it says now.
pub(crate) fn staged_change(
    delegate: &QrateTableDelegate,
    row: usize,
    col: usize,
    cx: &App,
) -> Option<CellChange> {
    find_change(delegate, row, col, cx).map(|(_, _, change)| change)
}

/// Apply or reject the one change staged on a cell; the agent's other changes stay staged.
pub(crate) fn settle_cell_change(row: usize, col: usize, apply: bool, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    let delegate = state.read(cx).delegate();
    let column = delegate.column_name(col);
    let Some((source, kind, change)) = find_change(delegate, row, col, cx) else {
        return;
    };
    crate::capabilities::record(
        match apply {
            true => "change_applied",
            false => "change_rejected",
        },
        agent_name(&source),
        format!("{column} row {}", row + 1),
        cx,
    );
    if let StagedKind::Rename(row_id) = kind {
        if !apply {
            if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(&source) {
                batch.renames.retain(|rename| rename.row != row_id);
            }
            publish(&source, cx);
        } else if let Err(why) = apply_some_renames(&source, Some(row_id), cx) {
            tell(format!("The file was not renamed: {why}"), cx);
        }
        return;
    }
    let mut relink = false;
    if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(&source) {
        match kind {
            StagedKind::Fix => {
                batch.by_cell.remove(&(row, column.clone()));
                batch.findings.retain(|finding| {
                    finding.location.row != Some(row)
                        || finding.location.column.as_ref() != Some(&column)
                });
            }
            StagedKind::Rewrite => {
                if let Some(rewrite) = batch.rewrites.get_mut(&column) {
                    relink = rewrite.relink;
                    rewrite.rows.remove(&row);
                    if rewrite.rows.is_empty() {
                        batch.rewrites.remove(&column);
                    }
                }
            }
            StagedKind::Rename(_) => {}
        }
    }
    publish(&source, cx);
    if apply {
        let origin = Origin::Agent {
            agent: change.agent.to_string(),
            fix: format!("Apply a change to {column}"),
        };
        write(vec![(row, col, change.after)], relink, origin, cx);
    }
}

/// How many changes one agent has staged on cells and files.
fn staged_count(batch: &Batch) -> usize {
    batch.by_cell.len()
        + batch
            .rewrites
            .values()
            .map(|rewrite| rewrite.rows.len())
            .sum::<usize>()
        + batch.renames.len()
        + batch.adds.len()
        + batch.deletes.as_ref().map_or(0, |(rows, _)| rows.len())
}

/// Append rows given as column name → value, as one undo step. Returns the new source rows.
pub(crate) fn append_rows(
    rows: &[BTreeMap<String, String>],
    origin: Origin,
    cx: &mut App,
) -> Result<Vec<usize>, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let width = delegate.column_count();
    let mut values = Vec::with_capacity(rows.len());
    for fields in rows {
        let mut cells = vec![SharedString::default(); width];
        for (name, value) in fields {
            let col = delegate.data_col(name).ok_or(RequestError::UnknownColumn)?;
            cells[col] = value.clone().into();
        }
        values.push(cells);
    }
    let at = delegate.row_count();
    let added = state.update(cx, |state, cx| {
        let added = state
            .delegate_mut()
            .append_spreadsheet_rows(values, origin.clone());
        let rows: Vec<usize> = (at..at + added).collect();
        crate::photos::refresh(state, Some(&rows), cx);
        state.refresh(cx);
        cx.emit(crate::TableChanged);
        cx.notify();
        added
    });
    let row_ids = state.read(cx).delegate().row_ids().to_vec();
    diagnostics::Diagnostics::align_note_rows(DATASET_MAIN, &row_ids, origin, cx);
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    crate::revalidate_now(cx);
    crate::autosave(cx);
    Ok((at..at + added).collect())
}

/// Stage rows to add and rows to delete as part of a change set; nothing changes until the
/// archivist applies it.
pub(crate) fn stage_rows(
    agent: &str,
    adds: Vec<BTreeMap<String, String>>,
    deletes: Option<(Vec<usize>, String)>,
    cx: &mut App,
) -> Result<(), RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    for (name, _) in adds.iter().flatten() {
        delegate.data_col(name).ok_or(RequestError::UnknownColumn)?;
    }
    let deletes = match deletes {
        Some((rows, reason)) => Some((
            rows.iter()
                .map(|row| delegate.row_id(*row).ok_or(RequestError::RowOutOfRange))
                .collect::<Result<Vec<_>, _>>()?,
            reason,
        )),
        None => None,
    };
    let source = agent_source(agent);
    let batch = cx
        .default_global::<StagedFixes>()
        .0
        .entry(source.clone())
        .or_default();
    batch.adds = adds;
    batch.deletes = deletes;
    batch.atomic = true;
    publish(&source, cx);
    Ok(())
}

/// Apply everything one agent staged: its cell values as one undo step, then its file renames as
/// another. Cells and files changed since the agent looked are skipped.
pub(crate) fn apply_all(source: &SharedString, cx: &mut App) {
    let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) else {
        return;
    };
    let Some((rows, reason)) = batch.deletes.clone() else {
        return apply_now(source, None, cx);
    };
    // Deleting always asks, even inside a change set the archivist chose to apply.
    let asked = capability::Host::confirm(
        &crate::capabilities::TableHost,
        capability::Confirmation {
            agent: agent_name(source).to_owned().into(),
            destructive: true,
            title: format!("Delete {} rows as part of its change set", rows.len()),
            reason: reason.clone(),
            details: Vec::new(),
        },
        cx,
    );
    let source = source.clone();
    cx.spawn(async move |cx| {
        let answer = asked.await;
        cx.update(|cx| match answer {
            Ok(()) => apply_now(&source, Some(rows), cx),
            Err(_) => tell(
                "Nothing was applied: the deletions were declined".into(),
                cx,
            ),
        })
    })
    .detach();
}

fn apply_now(source: &SharedString, deletes: Option<Vec<settings::project::RowId>>, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    let mark = state.read(cx).delegate().undo_depth();
    let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) else {
        return;
    };
    let (atomic, adds) = (batch.atomic, std::mem::take(&mut batch.adds));
    batch.deletes = None;
    batch.atomic = false;
    let fixes = std::mem::take(&mut batch.by_cell);
    let rewrites = std::mem::take(&mut batch.rewrites);
    batch.findings.clear();
    let has_renames = !batch.renames.is_empty();
    publish(source, cx);

    let delegate = state.read(cx).delegate();
    let total = fixes.len() + rewrites.values().map(|r| r.rows.len()).sum::<usize>();
    let mut cells: BTreeMap<(usize, usize), SharedString> = BTreeMap::new();
    for ((row, column), fixes) in &fixes {
        if let Some(col) = delegate.data_col(column)
            && let Some(text) = delegate.cell(*row, col)
            && let Some(fix) = fixes.iter().find(|fix| fix.expected == *text)
        {
            cells.insert((*row, col), fix.replacement.clone());
        }
    }
    let mut links = Vec::new();
    for (column, rewrite) in &rewrites {
        let Some(col) = delegate.data_col(column) else {
            continue;
        };
        for (row, (before, after)) in &rewrite.rows {
            if delegate.cell(*row, col) != Some(before) {
                continue;
            }
            match rewrite.relink {
                true => links.push((*row, col, after.clone())),
                false => {
                    cells.insert((*row, col), after.clone());
                }
            }
        }
    }
    let (applied, stale) = (cells.len() + links.len(), total - cells.len() - links.len());
    let agent = agent_name(source).to_owned();
    if applied > 0 {
        let origin = Origin::Agent {
            agent: agent.clone(),
            fix: format!("Apply {applied} staged changes"),
        };
        let cells = cells
            .into_iter()
            .map(|((row, col), text)| (row, col, text))
            .collect();
        crate::write_cells(cells, origin.clone(), cx);
        crate::write_links(links, origin, cx);
    }
    let mut message = format!("{applied} applied, {stale} skipped as stale");
    if has_renames {
        message = match apply_renames(source, cx) {
            Ok((renamed, stale)) => {
                format!("{message}; {renamed} files renamed, {stale} skipped as stale")
            }
            Err(why) => format!("{message}; no file was renamed: {why}"),
        };
    }
    let origin = Origin::Agent {
        agent: agent.clone(),
        fix: "Apply a change set".into(),
    };
    if !adds.is_empty() {
        match append_rows(&adds, origin.clone(), cx) {
            Ok(rows) => message = format!("{message}; {} rows added", rows.len()),
            Err(why) => message = format!("{message}; no row was added: {why}"),
        }
    }
    if let Some(ids) = deletes {
        let rows: Vec<usize> = ids
            .iter()
            .filter_map(|id| state.read(cx).delegate().row_of(*id))
            .collect();
        message = format!("{message}; {} rows deleted", rows.len());
        crate::structural(crate::Structural::DeleteRows(rows), origin, cx);
    }
    if atomic {
        state.update(cx, |state, _| state.delegate_mut().squash_undo_since(mark));
    }
    log::info!("{agent}: {message}");
    crate::capabilities::record("draft_applied", &agent, message.clone(), cx);
    tell(message, cx);
}

/// Drop everything one agent staged, touching no cell or file.
pub(crate) fn discard_all(source: &SharedString, cx: &mut App) {
    if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) {
        *batch = Batch::default();
    }
    publish(source, cx);
    crate::capabilities::record("draft_discarded", agent_name(source), String::new(), cx);
}

/// The strip above the grid while an agent has changes staged: what they are, and the two
/// whole-batch answers. Each change is also marked on its own cell.
pub fn render_staged_bar(cx: &mut App) -> AnyElement {
    let batches: Vec<(SharedString, usize)> = cx
        .try_global::<StagedFixes>()
        .map(|staged| {
            let mut batches: Vec<_> = staged
                .0
                .iter()
                .map(|(source, batch)| (source.clone(), staged_count(batch)))
                .filter(|(_, count)| *count > 0)
                .collect();
            batches.sort();
            batches
        })
        .unwrap_or_default();
    if batches.is_empty() {
        return Empty.into_any_element();
    }
    let (warning, border) = (cx.theme().warning, cx.theme().border);
    v_flex()
        .flex_none()
        .children(batches.into_iter().map(|(source, count)| {
            let agent = agent_name(&source).to_owned();
            let changes = match count {
                1 => "1 change".to_owned(),
                n => format!("{n} changes"),
            };
            let (apply, discard) = (source.clone(), source.clone());
            h_flex()
                .gap_2()
                .items_center()
                .px_2()
                .py_1p5()
                .border_b_1()
                .border_color(border)
                .bg(warning.opacity(0.12))
                .child(Icon::new(IconName::Inbox).small().text_color(warning))
                .child(div().flex_1().min_w_0().text_sm().truncate().child(format!(
                    "{agent} staged {changes}, marked in the table. Right-click a marked \
                             cell to apply or reject just that one."
                )))
                .child(
                    Button::new(SharedString::from(format!("staged-apply-{source}")))
                        .small()
                        .label("Apply all")
                        .on_click(move |_, _, cx| apply_all(&apply, cx)),
                )
                .child(
                    Button::new(SharedString::from(format!("staged-discard-{source}")))
                        .ghost()
                        .small()
                        .label("Discard")
                        .on_click(move |_, _, cx| discard_all(&discard, cx)),
                )
        }))
        .into_any_element()
}

/// Make one change to the table's shape at once, as one undo step credited to the agent.
/// Say what a bulk action did, over whichever window is active.
pub(crate) fn tell(message: String, cx: &mut App) {
    in_active_window(cx, move |window, cx| {
        window.push_notification(Notification::new().message(message), cx)
    });
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note on `note.rs`'s test module.
    use ai::agent::{
        Finding, Overview, ProgramOutput, ProjectSummary, Query, QueryResult, QuerySource, Rename,
        RequestError, RowPlacement, TableRevision, Thumbnail, ThumbnailRequest,
    };
    use diagnostics::{DATASET_MAIN, Location};
    use gpui::{AppContext as _, TestAppContext};
    use serde::{Deserialize, Serialize};

    use gpui::SharedString;

    use crate::agent::{agent_source, apply_renames, apply_rewrite, apply_staged, staged_actions};
    use crate::{TablePanel, TableStateHandle};

    /// The requests as the CLI has always put them on the wire, so these tests also hold the CLI's
    /// JSON to its word through the capability dispatcher.
    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    #[serde(tag = "method", content = "params", rename_all = "snake_case")]
    enum Request {
        Overview,
        Query(Query),
        ProgramSave {
            source: String,
        },
        ProgramRun {
            revision: TableRevision,
            args: serde_json::Value,
        },
        Thumbnails {
            items: Vec<ThumbnailRequest>,
        },
        StageFindings {
            revision: TableRevision,
            findings: Vec<Finding>,
        },
        StageColumnRewrite {
            revision: TableRevision,
            column: String,
            values: Vec<(usize, String)>,
            note: Option<String>,
        },
        StageRenames {
            revision: TableRevision,
            renames: Vec<Rename>,
        },
        Edit {
            revision: TableRevision,
            #[serde(flatten)]
            edit: TableEdit,
        },
        Delete {
            revision: TableRevision,
            #[serde(default)]
            rows: Vec<usize>,
            column: Option<String>,
            reason: String,
        },
    }

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum TableEdit {
        IndentRow {
            row: usize,
        },
        MoveRow {
            row: usize,
            target: usize,
            placement: RowPlacement,
        },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum ResultSet {
        Overview(Overview),
        Query(QueryResult),
        ProgramSaved {
            version: u64,
            hash: String,
        },
        ProgramRun(ProgramOutput),
        Thumbnails {
            items: Vec<Thumbnail>,
        },
        Staged {
            accepted: usize,
            stale: Vec<usize>,
        },
        StagedRewrite {
            accepted: usize,
            unchanged: Vec<usize>,
        },
        StagedRenames {
            accepted: usize,
        },
        Edited {
            detail: String,
            rows: Vec<usize>,
        },
        Deleted {
            count: usize,
        },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Response {
        revision: TableRevision,
        #[serde(flatten)]
        result: ResultSet,
    }

    /// The method and parameters `app_control` would dispatch for `request`.
    fn wire(request: &Request) -> (String, serde_json::Value) {
        let wire = serde_json::to_value(request).unwrap();
        let mut name = wire["method"].as_str().unwrap().to_owned();
        let mut params = wire.get("params").cloned().unwrap_or_default();
        if name == "edit" {
            name = params
                .as_object_mut()
                .and_then(|params| params.remove("op"))
                .and_then(|op| op.as_str().map(str::to_owned))
                .unwrap();
        }
        (name, params)
    }

    struct Registered;
    impl gpui::Global for Registered {}

    fn register(cx: &mut gpui::App) {
        if cx.has_global::<Registered>() {
            return;
        }
        cx.set_global(Registered);
        capability::init(cx);
        crate::capabilities::init(cx);
    }

    fn respond_as(
        agent: &str,
        request: Request,
        cx: &mut gpui::App,
    ) -> Result<Response, RequestError> {
        let (name, params) = wire(&request);
        call(agent, &name, params, cx)
    }

    fn call(
        agent: &str,
        name: &str,
        params: serde_json::Value,
        cx: &mut gpui::App,
    ) -> Result<Response, RequestError> {
        register(cx);
        let task = capability::dispatch(
            name,
            params,
            capability::Caller::Agent(agent.to_owned().into()),
            cx,
        )
        .task;
        let answer =
            capability::now(task).unwrap_or_else(|_| panic!("{name} did not answer at once"))?;
        Ok(serde_json::from_value(answer.value).unwrap())
    }

    fn respond_to_agent(request: Request, cx: &mut gpui::App) -> Result<Response, RequestError> {
        respond_as("pi", request, cx)
    }

    /// A call that waits on the archivist, answered once they have.
    fn respond_later(
        request: Request,
        cx: &mut gpui::App,
    ) -> gpui::Task<Result<Response, RequestError>> {
        register(cx);
        let (name, params) = wire(&request);
        let task =
            capability::dispatch(&name, params, capability::Caller::Agent("pi".into()), cx).task;
        let answer = |answer: Result<capability::Answer, RequestError>| {
            answer.map(|answer| serde_json::from_value(answer.value).unwrap())
        };
        match capability::now(task) {
            Ok(done) => gpui::Task::ready(answer(done)),
            Err(task) => cx
                .background_executor()
                .spawn(async move { answer(task.await) }),
        }
    }

    fn cell(row: usize, cx: &mut gpui::App) -> serde_json::Value {
        let ResultSet::Query(page) = respond_to_agent(
            Request::Query(Query {
                source: QuerySource::Rows { rows: vec![row] },
                select: vec!["Title".into()],
                limit: 1,
                ..Query::default()
            }),
            cx,
        )
        .unwrap()
        .result
        else {
            panic!("rows answered with another result set");
        };
        page.items[0].last().cloned().unwrap()
    }

    fn project() -> settings::project::CurrentProject {
        settings::project::CurrentProject {
            file: std::env::temp_dir().join("qrate-agent-smoke.qrate"),
            data: settings::project::ProjectData {
                name: "T".into(),
                columns: vec![settings::project::ProjectColumn {
                    name: "Title".into(),
                    data_type: "Title".into(),
                    notes: "The primary display title".into(),
                }],
                headers: vec!["Title".into(), "Medium".into()],
                rows: vec![
                    vec!["Harvest".into(), "Film".into()],
                    vec!["Wharf".into(), "Video".into()],
                    vec!["Cannery".into(), "Film".into()],
                ],
                row_ids: vec![1, 2, 3],
                values: Default::default(),
            },
        }
    }

    fn open(cx: &mut TestAppContext) {
        cx.update(|cx| {
            register(cx);
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });
        cx.add_window_view(TablePanel::new);
    }

    #[gpui::test]
    fn no_open_project_does_not_leak_table_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                respond_to_agent(Request::Overview, cx),
                Err(RequestError::ProjectUnavailable)
            );
        });
    }

    /// The end of the agent contract an agent actually grades its review on: real cell values out of the
    /// live delegate, not the shape of the protocol.
    #[gpui::test]
    fn the_live_table_answers_with_its_own_contents(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            assert!(matches!(
                respond_to_agent(Request::Overview, cx).unwrap().result,
                ResultSet::Overview(Overview {
                    project: ProjectSummary {
                        row_count: 3,
                        column_count: 2,
                        ..
                    },
                    ..
                })
            ));

            let ResultSet::Overview(Overview { columns, .. }) =
                respond_to_agent(Request::Overview, cx).unwrap().result
            else {
                panic!("columns answered with another result set");
            };
            let names: Vec<_> = columns.iter().map(|column| column.name.as_str()).collect();
            assert_eq!(names, ["Title", "Medium"]);
            assert_eq!(columns[0].data_type, "Title");
            assert_eq!(columns[0].notes, "The primary display title");

            let ResultSet::Query(page) = respond_to_agent(
                Request::Query(Query {
                    source: QuerySource::Search {
                        text: "video".into(),
                    },
                    limit: 10,
                    ..Query::default()
                }),
                cx,
            )
            .unwrap()
            .result
            else {
                panic!("search answered with another result set");
            };
            assert_eq!(
                page.items.len(),
                1,
                "case-insensitive search matched cell values"
            );
            assert_eq!(page.items[0][0], 1);
            assert_eq!(page.items[0][1], "Wharf");

            let ResultSet::Query(distinct) = respond_to_agent(
                Request::Query(Query {
                    distinct: Some("Medium".into()),
                    limit: 20,
                    ..Query::default()
                }),
                cx,
            )
            .unwrap()
            .result
            else {
                panic!("distinct answered with another result set")
            };
            assert_eq!(distinct.fields, ["Medium"]);
            assert_eq!(
                distinct.items,
                [
                    vec![serde_json::Value::from("Film")],
                    vec![serde_json::Value::from("Video")]
                ]
            );
        });
    }

    /// A filtered view is what the archivist sees, but an index that means different rows
    /// depending on the filter would make every quoted finding unfindable.
    #[gpui::test]
    fn a_filter_does_not_move_the_row_indices_an_agent_is_given(cx: &mut TestAppContext) {
        open(cx);

        let state = cx.update(|cx| {
            cx.try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .expect("the panel publishes its state handle")
        });
        // Keep only "Film" — source rows 0 and 2, so view row 1 is source row 2.
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.delegate_mut().set_column_kept(1, &["Film".into()]);
                cx.notify();
            });
            state.update(cx, |state, cx| state.set_selected_row(1, cx));
        });
        cx.run_until_parked();

        cx.update(|cx| {
            assert!(
                matches!(
                    respond_to_agent(Request::Overview, cx).unwrap().result,
                    ResultSet::Overview(Overview {
                        project: ProjectSummary { row_count: 3, .. },
                        ..
                    })
                ),
                "the filter hid a row from the count"
            );
            let ResultSet::Query(page) = respond_to_agent(
                Request::Query(Query {
                    source: QuerySource::SelectedRows,
                    limit: 20,
                    ..Query::default()
                }),
                cx,
            )
            .unwrap()
            .result
            else {
                panic!("rows answered with another result set");
            };
            assert_eq!(page.items[0][0], 2);
            assert_eq!(page.items[0][1], "Cannery");
        });
    }

    fn finding(row: usize, column: &str, expected: &str, replacement: &str) -> Finding {
        Finding {
            row,
            column: column.into(),
            severity: ai::agent::Severity::Warning,
            message: "the medium contradicts the title".into(),
            expected: expected.into(),
            replacement: Some(replacement.into()),
        }
    }

    /// Staging is the whole of the agent's power over the data: a finding reaches the Problems
    /// panel and its correction reaches the Fixes menu, but the cell keeps the text it had. A
    /// draft judged against text the cell no longer carries is dropped rather than published,
    /// because that is a cell nobody reviewed.
    #[gpui::test]
    fn staged_findings_are_drafts_and_stale_ones_are_dropped(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            let staged = respond_to_agent(
                Request::StageFindings {
                    revision: TableRevision(0),
                    findings: vec![
                        finding(0, "Title", "Harvest", "Harvest, 1962"),
                        finding(1, "Medium", "Kinescope", "Video"),
                        finding(99, "Title", "", "Nowhere"),
                    ],
                },
                cx,
            )
            .unwrap()
            .result;
            assert_eq!(
                staged,
                ResultSet::Staged {
                    accepted: 1,
                    stale: vec![1, 2]
                },
                "row 1's Medium says Video, not Kinescope, and row 99 does not exist"
            );

            let published: Vec<_> =
                diagnostics::Diagnostics::at(DATASET_MAIN, Some(0), Some("Title"), cx).collect();
            assert_eq!(published.len(), 1);
            assert_eq!(published[0].source.key(), agent_source("pi"));
            assert_eq!(published[0].severity, diagnostics::Severity::Warning);

            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(1),
                column: Some("Title".into()),
            };
            let offered = diagnostics::fixes::at_subject(&location, "Harvest", None, None, cx);
            assert_eq!(
                offered.len(),
                1,
                "the fix is offered against the judged text"
            );
            assert_eq!(offered[0].replacement, "Harvest, 1962");
            let ResultSet::Query(page) = respond_to_agent(
                Request::Query(Query {
                    source: QuerySource::Rows { rows: vec![0] },
                    limit: 20,
                    ..Query::default()
                }),
                cx,
            )
            .unwrap()
            .result
            else {
                panic!("rows answered with another result set");
            };
            assert_eq!(
                page.items[0][1], "Harvest",
                "staging a replacement must not write the cell"
            );
            assert!(
                diagnostics::fixes::at_subject(
                    &location,
                    "Harvest, retitled by hand",
                    None,
                    None,
                    cx
                )
                .is_empty(),
                "a cell edited since staging is offered nothing"
            );
        });
    }

    /// A second batch replaces the first rather than stacking beside it, so an agent that re-reads
    /// and re-stages retracts what it no longer stands by. An empty batch is how it retracts all.
    #[gpui::test]
    fn re_staging_replaces_the_previous_batch(cx: &mut TestAppContext) {
        open(cx);

        let stage = |findings: Vec<Finding>, cx: &mut gpui::App| {
            respond_to_agent(
                Request::StageFindings {
                    revision: TableRevision(0),
                    findings,
                },
                cx,
            )
            .unwrap()
        };
        let agent_findings = |cx: &gpui::App| {
            diagnostics::Diagnostics::all(cx)
                .iter()
                .filter(|d| d.source.key() == agent_source("pi"))
                .count()
        };

        cx.update(|cx| {
            stage(
                vec![
                    finding(0, "Title", "Harvest", "Harvest, 1962"),
                    finding(1, "Title", "Wharf", "Wharf, 1958"),
                ],
                cx,
            );
            assert_eq!(agent_findings(cx), 2);

            stage(vec![finding(0, "Title", "Harvest", "Harvest, 1962")], cx);
            assert_eq!(agent_findings(cx), 1, "the second batch replaced the first");

            stage(Vec::new(), cx);
            assert_eq!(agent_findings(cx), 0, "an empty batch retracts everything");
        });
    }
    fn stage(findings: Vec<Finding>, cx: &mut gpui::App) {
        respond_to_agent(
            Request::StageFindings {
                revision: TableRevision(0),
                findings,
            },
            cx,
        )
        .unwrap();
    }

    fn agent_findings(cx: &gpui::App) -> usize {
        diagnostics::Diagnostics::all(cx)
            .iter()
            .filter(|d| d.source.key() == agent_source("pi"))
            .count()
    }

    fn many_rows(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            let mut project = project();
            project.data.rows = (0..500)
                .map(|row| vec![format!("old {row}"), "Film".into()])
                .collect();
            project.data.row_ids = (1..=500).collect();
            cx.set_global(project);
        });
        cx.add_window_view(TablePanel::new);
    }

    /// The whole point of the batch action: one click, one undo step, however many cells.
    #[gpui::test]
    fn five_hundred_fixes_apply_as_one_step_and_one_undo_restores_them(cx: &mut TestAppContext) {
        many_rows(cx);

        cx.update(|cx| {
            for chunk in 0..3 {
                let rows = (chunk * 200..500.min((chunk + 1) * 200)).collect::<Vec<_>>();
                let findings = rows
                    .iter()
                    .map(|&row| finding(row, "Title", &format!("old {row}"), &format!("new {row}")))
                    .collect();
                respond_to_agent_as(&format!("pi-{chunk}"), findings, cx);
            }
            let labels: Vec<SharedString> = staged_actions(&agent_source("pi-0"), cx)
                .into_iter()
                .map(|action| action.label)
                .collect();
            assert_eq!(labels, ["Apply all 200 fixes from pi-0", "Discard all"]);
        });
        // One agent's batch is capped at 200 findings, so 500 in one step is one agent's batch
        // built directly: three agents would be three steps.
        cx.update(|cx| {
            for chunk in 0..3 {
                super::discard_staged(&agent_source(&format!("pi-{chunk}")), cx);
            }
            let by_cell = (0..500)
                .map(|row| {
                    (
                        (row, SharedString::from("Title")),
                        vec![super::StagedFix {
                            expected: format!("old {row}").into(),
                            replacement: format!("new {row}").into(),
                        }],
                    )
                })
                .collect();
            cx.default_global::<super::StagedFixes>().0.insert(
                agent_source("pi"),
                super::Batch {
                    by_cell,
                    ..Default::default()
                },
            );
            assert_eq!(apply_staged(&agent_source("pi"), cx), (500, 0));
            assert_eq!(cell(0, cx), "new 0");
            assert_eq!(cell(499, cx), "new 499");

            crate::history_step(false, cx);
            assert_eq!(cell(0, cx), "old 0");
            assert_eq!(cell(250, cx), "old 250");
            assert_eq!(cell(499, cx), "old 499");
            crate::history_step(false, cx);
            assert_eq!(cell(0, cx), "old 0", "the batch was one step, not several");

            crate::history_step(true, cx);
            assert_eq!(cell(499, cx), "new 499", "redo restores the whole batch");
        });
    }

    fn respond_to_agent_as(agent: &str, findings: Vec<Finding>, cx: &mut gpui::App) {
        respond_as(
            agent,
            Request::StageFindings {
                revision: TableRevision(0),
                findings,
            },
            cx,
        )
        .unwrap();
    }

    /// A cell the archivist edited after staging is a cell nobody reviewed: it keeps its text, and
    /// the count says so. Applying also retires the batch and its findings.
    #[gpui::test]
    fn a_stale_cell_is_skipped_and_counted(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            stage(
                vec![
                    finding(0, "Title", "Harvest", "Harvest, 1962"),
                    finding(1, "Title", "Wharf", "Wharf, 1958"),
                ],
                cx,
            );
            crate::write_cells(
                vec![(1, 0, "Wharf, retitled by hand".into())],
                settings::history::Origin::Typed,
                cx,
            );

            assert_eq!(apply_staged(&agent_source("pi"), cx), (1, 1));
            assert_eq!(cell(0, cx), "Harvest, 1962");
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .unwrap();
            let origin = &state
                .read(cx)
                .delegate()
                .unsaved_history()
                .last()
                .unwrap()
                .origin;
            assert_eq!(
                settings::history::credit(Some("rk"), origin).as_deref(),
                Some("rk using pi"),
                "the history names the agent beside whoever accepted its fixes"
            );
            assert_eq!(cell(1, cx), "Wharf, retitled by hand");
            assert_eq!(agent_findings(cx), 0);
            assert!(staged_actions(&agent_source("pi"), cx).is_empty());
        });
    }

    #[gpui::test]
    fn discarding_leaves_the_cells_untouched(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            stage(vec![finding(0, "Title", "Harvest", "Harvest, 1962")], cx);
            let discard = staged_actions(&agent_source("pi"), cx)
                .into_iter()
                .find(|action| action.label == "Discard all")
                .expect("a staged batch can be discarded");
            discard.apply(cx);

            assert_eq!(cell(0, cx), "Harvest");
            assert_eq!(agent_findings(cx), 0);
            assert_eq!(apply_staged(&agent_source("pi"), cx), (0, 0));
        });
    }

    /// One agent re-staging must not retract what another agent staged.
    #[gpui::test]
    fn agents_keep_separate_batches(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            stage(vec![finding(0, "Title", "Harvest", "Harvest, 1962")], cx);
            respond_to_agent_as(
                "other",
                vec![finding(1, "Title", "Wharf", "Wharf, 1958")],
                cx,
            );
            respond_to_agent_as("other", Vec::new(), cx);

            assert_eq!(agent_findings(cx), 1);
            assert_eq!(apply_staged(&agent_source("pi"), cx), (1, 0));
        });
    }

    /// A staged row is a source row, so a filter or a sort that reorders the view must not
    /// redirect the write.
    #[gpui::test]
    fn applying_under_a_filter_writes_the_source_rows(cx: &mut TestAppContext) {
        open(cx);

        let state = cx.update(|cx| {
            cx.try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .expect("the panel publishes its state handle")
        });
        cx.update(|cx| {
            stage(
                vec![
                    finding(1, "Title", "Wharf", "Wharf, 1958"),
                    finding(2, "Title", "Cannery", "Cannery, 1949"),
                ],
                cx,
            );
            // Keep only "Film" — source rows 0 and 2, so view row 1 is source row 2.
            state.update(cx, |state, cx| {
                state.delegate_mut().set_column_kept(1, &["Film".into()]);
                cx.notify();
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            assert_eq!(apply_staged(&agent_source("pi"), cx), (2, 0));
            assert_eq!(cell(0, cx), "Harvest");
            assert_eq!(cell(1, cx), "Wharf, 1958", "a hidden row is still written");
            assert_eq!(cell(2, cx), "Cannery, 1949");
        });
    }
    fn rewrite(column: &str, values: &[(usize, &str)], cx: &mut gpui::App) -> Request {
        let revision = respond_to_agent(Request::Overview, cx).unwrap().revision;
        serde_json::from_value(serde_json::json!({
            "method": "stage_column_rewrite",
            "params": {
                "revision": revision,
                "column": column,
                "values": values,
                "note": "ids from mapping.csv",
            }
        }))
        .unwrap()
    }

    /// From the wire shape to the grid: a rewrite is one Problems entry however many rows it
    /// names, changes nothing until applied, and then lands as one undo step.
    #[gpui::test]
    fn a_column_rewrite_round_trips_from_json_to_one_undoable_apply(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            let request = rewrite("Title", &[(0, "H-1"), (1, "Wharf"), (2, "C-3")], cx);
            assert_eq!(
                respond_to_agent(request, cx).unwrap().result,
                ResultSet::StagedRewrite {
                    accepted: 2,
                    unchanged: vec![1]
                }
            );
            assert_eq!(agent_findings(cx), 1, "one entry, not one per row");
            assert_eq!(cell(0, cx), "Harvest", "staging writes nothing");
            let labels: Vec<SharedString> = staged_actions(&agent_source("pi"), cx)
                .into_iter()
                .map(|action| action.label)
                .collect();
            assert_eq!(
                labels,
                [
                    "Apply: Rewrite Title: 2 rows (ids from mapping.csv)",
                    "Discard: Rewrite Title: 2 rows (ids from mapping.csv)",
                ]
            );

            crate::write_cells(
                vec![(2, 0, "Cannery, retitled by hand".into())],
                settings::history::Origin::Typed,
                cx,
            );
            assert_eq!(
                apply_rewrite(&agent_source("pi"), &"Title".into(), cx),
                (1, 1)
            );
            assert_eq!(cell(0, cx), "H-1");
            assert_eq!(cell(2, cx), "Cannery, retitled by hand");
            assert_eq!(agent_findings(cx), 0);

            crate::history_step(false, cx);
            assert_eq!(cell(0, cx), "Harvest");
        });
    }

    #[gpui::test]
    fn a_rewrite_the_table_cannot_place_is_refused(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            let request = rewrite("Identifier", &[(0, "H-1")], cx);
            assert_eq!(
                respond_to_agent(request, cx),
                Err(RequestError::UnknownColumn)
            );
            let request = rewrite("Title", &[(3, "H-1")], cx);
            assert_eq!(
                respond_to_agent(request, cx),
                Err(RequestError::RewriteRowOutOfRange)
            );
            let request = rewrite("Title", &[(1, "a"), (1, "b")], cx);
            assert_eq!(
                respond_to_agent(request, cx),
                Err(RequestError::DuplicateRewriteRow)
            );
            let Request::StageColumnRewrite {
                column,
                values,
                note,
                ..
            } = rewrite("Title", &[(0, "H-1")], cx)
            else {
                panic!("not a rewrite");
            };
            assert_eq!(
                respond_to_agent(
                    Request::StageColumnRewrite {
                        revision: TableRevision(u64::MAX),
                        column,
                        values,
                        note,
                    },
                    cx,
                ),
                Err(RequestError::StaleRevision)
            );
            assert_eq!(agent_findings(cx), 0);
        });
    }

    /// Staging the same column again is a correction, not a second rewrite; another column and
    /// the agent's findings are left alone.
    #[gpui::test]
    fn re_staging_replaces_the_rewrite_for_the_same_agent_and_column(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            stage(vec![finding(1, "Title", "Wharf", "Wharf, 1958")], cx);
            for (column, values) in [
                ("Title", vec![(0, "H-1"), (2, "C-3")]),
                ("Medium", vec![(1, "Tape")]),
                ("Title", vec![(0, "H-001")]),
            ] {
                let request = rewrite(column, &values, cx);
                respond_to_agent(request, cx).unwrap();
            }
            assert_eq!(agent_findings(cx), 3, "a finding and two rewrites");

            assert_eq!(
                apply_rewrite(&agent_source("pi"), &"Title".into(), cx),
                (1, 0)
            );
            assert_eq!(cell(0, cx), "H-001");
            assert_eq!(
                cell(2, cx),
                "Cannery",
                "the replaced rewrite's row is untouched"
            );
            assert_eq!(agent_findings(cx), 2);

            let request = rewrite("Medium", &[], cx);
            respond_to_agent(request, cx).unwrap();
            assert_eq!(agent_findings(cx), 1, "an empty rewrite retracts it");
        });
    }
    /// A project on disk: `<dir>/p.qrate`, with rows 0..3 naming `<dir>/photos/{1,2,3}.jpg` by
    /// their stems in a Filename column, each file holding its own first name.
    fn open_files(case: &str, cx: &mut TestAppContext) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("qrate-agent-renames").join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        for name in ["1", "2", "3"] {
            std::fs::write(dir.join(format!("photos/{name}.jpg")), name).unwrap();
        }
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            let mut project = project();
            project.file = dir.join("p.qrate");
            project.data.columns = vec![settings::project::ProjectColumn {
                name: "File".into(),
                data_type: "Filename".into(),
                notes: String::new(),
            }];
            project.data.headers = vec!["File".into(), "Title".into()];
            project.data.rows = vec![
                vec!["1".into(), "Harvest".into()],
                vec!["2".into(), "Wharf".into()],
                vec!["3".into(), "Cannery".into()],
            ];
            cx.set_global(project);
            settings::project::CurrentProject::set_text(
                settings::project::FILES_FOLDER_KEY,
                dir.join("photos").to_string_lossy().into_owned().into(),
                cx,
            );
        });
        cx.add_window_view(TablePanel::new);
        cx.run_until_parked();
        dir
    }

    fn renames(pairs: &[(usize, &str, &str)], cx: &mut gpui::App) -> Request {
        Request::StageRenames {
            revision: respond_to_agent(Request::Overview, cx).unwrap().revision,
            renames: pairs
                .iter()
                .map(|(row, from, to)| ai::agent::Rename {
                    row: *row,
                    from: (*from).into(),
                    to: (*to).into(),
                })
                .collect(),
        }
    }

    /// `(File cell, linked file's name)` for a row, straight from the delegate.
    fn linked(row: usize, cx: &gpui::App) -> (String, Option<String>) {
        let state = cx
            .try_global::<TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .unwrap();
        let delegate = state.read(cx).delegate();
        (
            delegate.cell(row, 0).unwrap().to_string(),
            delegate
                .row_image(row)
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned()),
        )
    }

    fn on_disk(dir: &std::path::Path, name: &str) -> String {
        std::fs::read_to_string(dir.join("photos").join(name)).unwrap_or_default()
    }

    /// Staging renames nothing; applying renames the file and the cell naming it together, and
    /// undo and redo carry the disk with the grid.
    #[gpui::test]
    fn a_staged_rename_moves_the_file_and_its_row_and_undoes(cx: &mut TestAppContext) {
        let dir = open_files("simple", cx);
        cx.update(|cx| {
            assert_eq!(linked(0, cx), ("1".into(), Some("1.jpg".into())));
            let request = renames(&[(0, "photos/1.jpg", "photos/AD-001.jpg")], cx);
            assert_eq!(
                respond_to_agent(request, cx).unwrap().result,
                ResultSet::StagedRenames { accepted: 1 }
            );
            assert_eq!(on_disk(&dir, "1.jpg"), "1", "staging renames nothing");
            assert_eq!(agent_findings(cx), 1);
            let labels: Vec<SharedString> = staged_actions(&agent_source("pi"), cx)
                .into_iter()
                .map(|action| action.label)
                .collect();
            assert_eq!(labels, ["Apply: Rename 1 files", "Discard: Rename 1 files"]);

            assert_eq!(apply_renames(&agent_source("pi"), cx), Ok((1, 0)));
            assert_eq!(on_disk(&dir, "AD-001.jpg"), "1");
            assert!(!dir.join("photos/1.jpg").exists());
            assert_eq!(agent_findings(cx), 0);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                linked(0, cx),
                ("AD-001".into(), Some("AD-001.jpg".into())),
                "the row follows its file"
            );
            crate::history_step(false, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(on_disk(&dir, "1.jpg"), "1", "undo renames the file back");
            assert!(!dir.join("photos/AD-001.jpg").exists());
            assert_eq!(linked(0, cx), ("1".into(), Some("1.jpg".into())));
            crate::history_step(true, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(on_disk(&dir, "AD-001.jpg"), "1");
            assert_eq!(linked(0, cx), ("AD-001".into(), Some("AD-001.jpg".into())));
        });
    }

    #[gpui::test]
    fn two_files_can_swap_names(cx: &mut TestAppContext) {
        let dir = open_files("swap", cx);
        cx.update(|cx| {
            let request = renames(
                &[
                    (0, "photos/1.jpg", "photos/2.jpg"),
                    (1, "photos/2.jpg", "photos/1.jpg"),
                ],
                cx,
            );
            respond_to_agent(request, cx).unwrap();
            assert_eq!(apply_renames(&agent_source("pi"), cx), Ok((2, 0)));
            assert_eq!(on_disk(&dir, "1.jpg"), "2");
            assert_eq!(on_disk(&dir, "2.jpg"), "1");
            assert_eq!(std::fs::read_dir(dir.join("photos")).unwrap().count(), 3);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(linked(0, cx), ("2".into(), Some("2.jpg".into())));
            assert_eq!(linked(1, cx), ("1".into(), Some("1.jpg".into())));
            crate::history_step(false, cx);
            assert_eq!(on_disk(&dir, "1.jpg"), "1");
            assert_eq!(on_disk(&dir, "2.jpg"), "2");
        });
    }

    #[gpui::test]
    fn unsound_renames_are_refused_whole(cx: &mut TestAppContext) {
        let dir = open_files("refused", cx);
        cx.update(|cx| {
            let refused = |pairs: &[(usize, &str, &str)], cx: &mut gpui::App| {
                let request = renames(pairs, cx);
                respond_to_agent(request, cx).unwrap_err()
            };
            assert_eq!(
                refused(
                    &[
                        (0, "photos/1.jpg", "photos/a.jpg"),
                        (1, "photos/2.jpg", "photos/a.jpg")
                    ],
                    cx
                ),
                RequestError::RenameTargetCollision("photos/a.jpg".into())
            );
            assert_eq!(
                refused(&[(0, "photos/1.jpg", "photos/3.jpg")], cx),
                RequestError::RenameTargetExists("photos/3.jpg".into())
            );
            assert_eq!(
                refused(&[(0, "photos/1.jpg", "../1.jpg")], cx),
                RequestError::RenameOutsideProject("../1.jpg".into())
            );
            let outside = std::env::temp_dir().join("elsewhere.jpg");
            assert!(matches!(
                refused(&[(0, "photos/1.jpg", outside.to_str().unwrap())], cx),
                RequestError::RenameOutsideProject(_)
            ));
            assert_eq!(
                refused(&[(1, "photos/1.jpg", "photos/a.jpg")], cx),
                RequestError::RenameSourceMismatch("photos/1.jpg".into()),
                "row 1 links 2.jpg"
            );
            assert_eq!(
                refused(&[(0, "photos/missing.jpg", "photos/a.jpg")], cx),
                RequestError::RenameSourceMismatch("photos/missing.jpg".into())
            );
            assert_eq!(agent_findings(cx), 0);
            assert_eq!(on_disk(&dir, "1.jpg"), "1");
        });
    }

    /// The walkthrough the feature exists for: an identifier rewrite and the matching file
    /// renames, staged together, applied one after the other, and put back by two undos. The
    /// renames, not the rewrite, change the file column, so the rows stay linked throughout.
    #[gpui::test]
    fn a_rewrite_and_its_renames_apply_and_two_undos_restore_everything(cx: &mut TestAppContext) {
        let dir = open_files("walkthrough", cx);
        let ids = ["AD-001", "AD-002", "AD-003"];
        cx.update(|cx| {
            let values: Vec<(usize, &str)> = ids.iter().copied().enumerate().collect();
            let request = rewrite("Title", &values, cx);
            respond_to_agent(request, cx).unwrap();
            let pairs: Vec<(usize, String, String)> = (0..3)
                .map(|row| {
                    (
                        row,
                        format!("photos/{}.jpg", row + 1),
                        format!("photos/{}.jpg", ids[row]),
                    )
                })
                .collect();
            let pairs: Vec<(usize, &str, &str)> = pairs
                .iter()
                .map(|(row, from, to)| (*row, from.as_str(), to.as_str()))
                .collect();
            let request = renames(&pairs, cx);
            respond_to_agent(request, cx).unwrap();
            assert_eq!(agent_findings(cx), 2, "one entry each");

            assert_eq!(
                apply_rewrite(&agent_source("pi"), &"Title".into(), cx),
                (3, 0)
            );
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                linked(0, cx),
                ("1".into(), Some("1.jpg".into())),
                "still linked"
            );
            assert_eq!(cell(0, cx), "AD-001");
            assert_eq!(apply_renames(&agent_source("pi"), cx), Ok((3, 0)));
        });
        cx.run_until_parked();
        cx.update(|cx| {
            for (row, id) in ids.iter().enumerate() {
                assert_eq!(linked(row, cx), ((*id).into(), Some(format!("{id}.jpg"))));
                assert_eq!(cell(row, cx), *id);
                assert_eq!(on_disk(&dir, &format!("{id}.jpg")), (row + 1).to_string());
            }
            crate::history_step(false, cx);
            crate::history_step(false, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            for (row, title) in ["Harvest", "Wharf", "Cannery"].iter().enumerate() {
                let name = (row + 1).to_string();
                assert_eq!(linked(row, cx), (name.clone(), Some(format!("{name}.jpg"))));
                assert_eq!(on_disk(&dir, &format!("{name}.jpg")), name);
                assert_eq!(cell(row, cx), *title);
            }
            assert_eq!(std::fs::read_dir(dir.join("photos")).unwrap().count(), 3);
        });
    }

    /// A file that moved after staging is left out, and the rest still go through.
    #[gpui::test]
    fn a_rename_whose_file_is_gone_is_skipped_as_stale(cx: &mut TestAppContext) {
        let dir = open_files("stale", cx);
        cx.update(|cx| {
            let request = renames(
                &[
                    (0, "photos/1.jpg", "photos/a.jpg"),
                    (1, "photos/2.jpg", "photos/b.jpg"),
                ],
                cx,
            );
            respond_to_agent(request, cx).unwrap();
            std::fs::remove_file(dir.join("photos/2.jpg")).unwrap();
            assert_eq!(apply_renames(&agent_source("pi"), cx), Ok((1, 1)));
            assert_eq!(on_disk(&dir, "a.jpg"), "1");
        });
    }

    /// The table in a window that can show dialogs and notifications, made the active one.
    fn open_in_window(cx: &mut TestAppContext) -> gpui::AnyWindowHandle {
        cx.update(|cx| {
            register(cx);
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });
        let window = cx.add_window(|window, cx| {
            let table = cx.new(|cx| TablePanel::new(window, cx));
            window.activate_window();
            gpui_component::Root::new(table, window, cx)
        });
        cx.run_until_parked();
        window.into()
    }

    fn revision(cx: &mut gpui::App) -> TableRevision {
        respond_to_agent(Request::Overview, cx).unwrap().revision
    }

    fn edit(op: serde_json::Value, cx: &mut gpui::App) -> Result<ResultSet, RequestError> {
        let mut params = op;
        params["revision"] = serde_json::json!(revision(cx).0);
        let name = params
            .as_object_mut()
            .and_then(|params| params.remove("op"))
            .and_then(|op| op.as_str().map(str::to_owned))
            .unwrap();
        call("pi", &name, params, cx).map(|response| response.result)
    }

    fn headers(cx: &gpui::App) -> Vec<String> {
        let state = cx
            .try_global::<TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .unwrap();
        let delegate = state.read(cx).delegate();
        (0..delegate.column_count())
            .map(|col| delegate.column_name(col).to_string())
            .collect()
    }

    fn last_change(cx: &gpui::App) -> Option<String> {
        let state = cx
            .try_global::<TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .unwrap();
        let origin = &state.read(cx).delegate().unsaved_history().last()?.origin;
        settings::history::credit(Some("rk"), origin)
            .map(|who| format!("{} · {who}", origin.label()))
    }

    #[gpui::test]
    fn autosave_keeps_agent_credit_and_notifies_history(cx: &mut TestAppContext) {
        let file = std::env::temp_dir().join("qrate-agent-saved-credit.qrate");
        let _ = std::fs::remove_file(&file);
        let mut project = project();
        project.file = file.clone();
        settings::project::create_project_file(
            &file,
            &settings::project::ProjectSpec {
                name: &project.data.name,
                source: "CSV",
                columns: &project.data.columns,
                headers: &project.data.headers,
                rows: &project.data.rows,
                ..Default::default()
            },
        )
        .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::project::ProjectPersistence {
                writer: Some(settings::project::ProjectSettingsWriter::start()),
            });
            cx.set_global(project);
        });
        cx.add_window_view(TablePanel::new);
        cx.run_until_parked();
        let saves = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = saves.clone();
        let _subscription = cx.update(|cx| {
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            cx.subscribe(&state, move |state, _: &crate::TableChanged, cx| {
                if state.read(cx).delegate().unsaved_history().is_empty() {
                    seen.set(seen.get() + 1);
                }
            })
        });
        for (operation, label) in [
            (
                serde_json::json!({"op":"rename_column","column":"Title","name":"Caption"}),
                "Rename column Title to Caption",
            ),
            (
                serde_json::json!({"op":"add_column","name":"Year"}),
                "Add column Year",
            ),
            (
                serde_json::json!({"op":"move_column","column":"Year","after":"Medium"}),
                "Move column Year",
            ),
            (
                serde_json::json!({"op":"add_rows","rows":[{"Caption":"New"}]}),
                "Add a row",
            ),
            (
                serde_json::json!({"op":"duplicate_row","row":0}),
                "Duplicate row 1",
            ),
            (
                serde_json::json!({"op":"indent_row","row":1}),
                "Indent row 2",
            ),
            (
                serde_json::json!({"op":"describe_column","column":"Caption","description":"Updated description"}),
                "Describe column Caption",
            ),
            (
                serde_json::json!({"op":"filter","column":"Year","keep":[""]}),
                "Filter Year",
            ),
        ] {
            cx.update(|cx| {
                edit(operation, cx).unwrap();
            });
            cx.run_until_parked();
            let saved = settings::history::entries_after(&file, 0).unwrap();
            assert_eq!(
                saved.last().unwrap().origin,
                settings::history::Origin::Agent {
                    agent: "pi".into(),
                    fix: label.into(),
                },
                "the saved entry must keep its agent, not just the pending entry"
            );
            let data = settings::project::load_project_file(&file).unwrap();
            for change in &saved.last().unwrap().changes {
                if let settings::history::Change::ColumnDescribed { column, after, .. } = change {
                    let stored = data
                        .columns
                        .iter()
                        .find(|entry| entry.name == *column)
                        .unwrap();
                    assert_eq!(
                        (&stored.data_type, &stored.notes),
                        (&after.data_type, &after.notes),
                        "metadata and its History must commit together, without waiting for the settings debounce"
                    );
                }
            }
        }
        assert!(
            saves.get() >= 8,
            "History hears when pending entries reach disk"
        );
        cx.update(|cx| {
            cx.global_mut::<settings::AppSettings>().values.insert(
                settings::AUTOSAVE_KEY.into(),
                settings::Val::Text("off".into()),
            );
            assert!(crate::describe_column(
                "Caption",
                None,
                Some("Manual description"),
                settings::history::Origin::Structure,
                cx
            ));
        });
        cx.run_until_parked();
        let before_save = settings::project::load_project_file(&file).unwrap();
        assert_eq!(
            before_save
                .columns
                .iter()
                .find(|column| column.name == "Caption")
                .unwrap()
                .notes,
            "Updated description",
            "manual metadata respects autosave off"
        );
        cx.update(|cx| crate::save_now(cx).unwrap());

        assert_eq!(
            settings::history::entries_after(&file, 0)
                .unwrap()
                .last()
                .unwrap()
                .origin,
            settings::history::Origin::Structure
        );
        let data = settings::project::load_project_file(&file).unwrap();
        assert_eq!(
            data.columns
                .iter()
                .find(|column| column.name == "Caption")
                .unwrap()
                .notes,
            "Manual description"
        );
        cx.update(|cx| crate::history_step(false, cx));
        cx.update(|cx| crate::save_now(cx).unwrap());
        cx.run_until_parked();
        let data = settings::project::load_project_file(&file).unwrap();
        assert_eq!(
            data.columns
                .iter()
                .find(|column| column.name == "Caption")
                .unwrap()
                .notes,
            "Updated description"
        );
    }

    #[gpui::test]
    fn agent_arrangement_descriptions_and_filters_have_reversible_history(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            let original = state.read(cx).delegate().row_structure();
            let revision_before = revision(cx);
            edit(serde_json::json!({"op":"indent_row","row":1}), cx).unwrap();
            assert_ne!(revision(cx), revision_before);
            assert_eq!(last_change(cx).as_deref(), Some("Indent row 2 · rk using pi"));
            let entry = state.read(cx).delegate().unsaved_history().last().unwrap();
            assert!(entry.changes.iter().any(|change| matches!(change, settings::history::Change::RowArranged { row: 2, .. })));
            crate::history_step(false, cx);
            assert_eq!(state.read(cx).delegate().row_structure(), original);
            crate::history_step(true, cx);
            assert_ne!(state.read(cx).delegate().row_structure(), original);
            crate::history_step(false, cx);

            edit(serde_json::json!({"op":"describe_column","column":"Title","data_type":"Date","description":"Date captured"}), cx).unwrap();
            assert_eq!(last_change(cx).as_deref(), Some("Describe column Title · rk using pi"));
            crate::history_step(false, cx);
            let restored = cx.global::<settings::project::CurrentProject>().data.columns.iter().find(|column| column.name == "Title").unwrap();
            assert_eq!((&*restored.data_type, &*restored.notes), ("Title", "The primary display title"));
            crate::history_step(true, cx);
            let restored = cx.global::<settings::project::CurrentProject>().data.columns.iter().find(|column| column.name == "Title").unwrap();
            assert_eq!((&*restored.data_type, &*restored.notes), ("Date", "Date captured"));

            let visible = state.read(cx).delegate().visible().to_vec();
            edit(serde_json::json!({"op":"filter","column":"Medium","keep":["Video"]}), cx).unwrap();
            assert_eq!(last_change(cx).as_deref(), Some("Filter Medium · rk using pi"));
            assert_ne!(state.read(cx).delegate().visible(), visible);
            crate::history_step(false, cx);
            assert_eq!(state.read(cx).delegate().visible(), visible);
            crate::history_step(true, cx);
            assert_ne!(state.read(cx).delegate().visible(), visible);
        });
    }

    /// An agent may change the table's shape directly; each change is one undo step credited to it.
    #[gpui::test]
    fn an_agent_changes_the_tables_shape_at_once(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            let added = edit(
                serde_json::json!({"op": "add_rows", "rows": [{"Title": "Pier"}, {"Medium": "Tape"}]}),
                cx,
            )
            .unwrap();
            assert_eq!(
                added,
                ResultSet::Edited {
                    detail: "Add 2 rows".into(),
                    rows: vec![3, 4]
                }
            );
            assert_eq!(cell(3, cx), "Pier");
            assert_eq!(last_change(cx).as_deref(), Some("Add 2 rows · rk using pi"));
            crate::history_step(false, cx);
            assert_eq!(
                respond_to_agent(Request::Overview, cx).unwrap().result,
                respond_to_agent(Request::Overview, cx).unwrap().result
            );
            assert!(
                matches!(
                    respond_to_agent(Request::Overview, cx).unwrap().result,
                    ResultSet::Overview(Overview { project: ProjectSummary { row_count: 3, .. }, .. })
                ),
                "one undo removes both added rows"
            );

            edit(serde_json::json!({"op": "add_column", "name": "Year", "after": "Title"}), cx).unwrap();
            assert_eq!(headers(cx), ["Title", "Year", "Medium"]);
            assert_eq!(
                edit(serde_json::json!({"op": "add_column", "name": "Medium"}), cx),
                Err(RequestError::ColumnNameTaken("Medium".into()))
            );
            edit(serde_json::json!({"op": "rename_column", "column": "Year", "name": "Date"}), cx).unwrap();
            edit(serde_json::json!({"op": "move_column", "column": "Date", "after": "Medium"}), cx).unwrap();
            assert_eq!(headers(cx), ["Title", "Medium", "Date"]);
            assert_eq!(last_change(cx).as_deref(), Some("Move column Date · rk using pi"));
            crate::history_step(false, cx);
            assert_eq!(headers(cx), ["Title", "Date", "Medium"], "one undo per change");

            edit(
                serde_json::json!({"op": "describe_column", "column": "Date", "data_type": "date", "description": "Year made"}),
                cx,
            )
            .unwrap();
            let declared = cx
                .global::<settings::project::CurrentProject>()
                .data
                .columns
                .iter()
                .find(|column| column.name == "Date")
                .map(|column| (column.data_type.clone(), column.notes.clone()));
            assert_eq!(declared, Some(("Date".into(), "Year made".into())));
            assert_eq!(
                edit(serde_json::json!({"op": "describe_column", "column": "Date", "data_type": "colour"}), cx),
                Err(RequestError::UnknownDataType("colour".into()))
            );

            assert!(matches!(
                edit(serde_json::json!({"op": "outdent_row", "row": 0}), cx),
                Err(RequestError::EditRefused(_))
            ));
            edit(serde_json::json!({"op": "indent_row", "row": 1}), cx).unwrap();
            edit(serde_json::json!({"op": "duplicate_row", "row": 2}), cx).unwrap();
            assert_eq!(cell(3, cx), "Cannery");
            assert_eq!(
                edit(serde_json::json!({"op": "duplicate_row", "row": 9}), cx),
                Err(RequestError::RowOutOfRange)
            );
            edit(serde_json::json!({"op": "filter", "column": "Medium", "keep": ["Video"]}), cx).unwrap();
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .unwrap();
            let visible = state.read(cx).delegate().visible().to_vec();
            assert!(visible.contains(&1), "the Video row shows");
            assert!(!visible.contains(&2) && !visible.contains(&3), "the Film rows are hidden");
            edit(serde_json::json!({"op": "filter", "column": "Medium"}), cx).unwrap();
            let medium = state.read(cx).delegate().data_col("Medium").unwrap();
            assert!(!state.read(cx).delegate().column_has_filter(medium), "the filter is cleared");
            assert_eq!(
                respond_to_agent(
                    Request::Edit {
                        revision: TableRevision(u64::MAX),
                        edit: TableEdit::IndentRow { row: 1 },
                    },
                    cx
                ),
                Err(RequestError::StaleRevision)
            );
        });
    }

    fn delete(rows: Vec<usize>, column: Option<&str>, cx: &mut gpui::App) -> Request {
        Request::Delete {
            revision: revision(cx),
            rows,
            column: column.map(Into::into),
            reason: "they duplicate row 0".into(),
        }
    }

    /// Deleting always asks the archivist, with the agent's reason, and only their answer decides.
    #[gpui::test]
    async fn a_delete_waits_for_the_archivist(cx: &mut TestAppContext) {
        use gpui_component::WindowExt as _;
        let window = open_in_window(cx);
        let asked = cx.update(|cx| {
            let request = delete(vec![1, 2], None, cx);
            respond_later(request, cx)
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert!(window.has_active_dialog(cx), "the archivist is asked");
        })
        .unwrap();
        cx.update(|cx| {
            let second = delete(vec![0], None, cx);
            assert!(matches!(
                respond_later(second, cx),
                task if task.is_ready()
            ));
            assert_eq!(cell(1, cx), "Wharf", "nothing is deleted before the answer");
            crate::capabilities::answer_pending_confirm(true, cx);
        });
        assert_eq!(asked.await.unwrap().result, ResultSet::Deleted { count: 2 });
        cx.update(|cx| {
            assert_eq!(headers(cx).len(), 2);
            assert!(matches!(
                respond_to_agent(Request::Overview, cx).unwrap().result,
                ResultSet::Overview(Overview {
                    project: ProjectSummary { row_count: 1, .. },
                    ..
                })
            ));
            assert_eq!(
                last_change(cx).as_deref(),
                Some("Delete 2 rows · rk using pi")
            );
            crate::history_step(false, cx);
            assert_eq!(cell(2, cx), "Cannery", "one undo brings both rows back");
        });

        let kept = cx.update(|cx| {
            let request = delete(Vec::new(), Some("Medium"), cx);
            respond_later(request, cx)
        });
        cx.run_until_parked();
        cx.update(|cx| crate::capabilities::answer_pending_confirm(false, cx));
        assert_eq!(kept.await, Err(RequestError::Declined));
        cx.update(|cx| assert_eq!(headers(cx), ["Title", "Medium"]));
    }

    /// A second delete while one is waiting is refused at once rather than queued behind it.
    #[gpui::test]
    async fn only_one_delete_waits_at_a_time(cx: &mut TestAppContext) {
        open_in_window(cx);
        let first = cx.update(|cx| {
            let request = delete(vec![0], None, cx);
            respond_later(request, cx)
        });
        cx.run_until_parked();
        let second = cx.update(|cx| {
            let request = delete(vec![1], None, cx);
            respond_later(request, cx)
        });
        assert_eq!(second.await, Err(RequestError::ConfirmPending));
        cx.update(|cx| crate::capabilities::answer_pending_confirm(false, cx));
        assert_eq!(first.await, Err(RequestError::Declined));
    }

    /// Staged values show on their own cells and can be settled one at a time, or all at once.
    #[gpui::test]
    fn staged_changes_show_on_their_cells_and_settle_one_by_one(cx: &mut TestAppContext) {
        let window = open_in_window(cx);
        cx.update(|cx| {
            let request = rewrite("Title", &[(0, "H-1"), (2, "C-3")], cx);
            respond_to_agent(request, cx).unwrap();
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .unwrap();
            let shown = |row: usize, cx: &gpui::App| {
                crate::agent::staged_change(state.read(cx).delegate(), row, 0, cx)
                    .map(|change| (change.before.to_string(), change.after.to_string()))
            };
            assert_eq!(shown(0, cx), Some(("Harvest".into(), "H-1".into())));
            assert_eq!(shown(1, cx), None);

            crate::agent::settle_cell_change(0, 0, true, cx);
            assert_eq!(cell(0, cx), "H-1");
            assert_eq!(shown(0, cx), None);
            assert_eq!(
                shown(2, cx),
                Some(("Cannery".into(), "C-3".into())),
                "the rest stay staged"
            );
            crate::agent::settle_cell_change(2, 0, false, cx);
            assert_eq!(cell(2, cx), "Cannery", "a rejected change writes nothing");
            assert_eq!(agent_findings(cx), 0, "nothing is left staged");

            stage(vec![finding(1, "Title", "Wharf", "Wharf, 1958")], cx);
            let request = rewrite("Medium", &[(0, "Nitrate")], cx);
            respond_to_agent(request, cx).unwrap();
            crate::agent::apply_all(&agent_source("pi"), cx);
            assert_eq!(cell(1, cx), "Wharf, 1958");
            crate::history_step(false, cx);
            assert_eq!(cell(1, cx), "Wharf", "applying everything is one undo step");
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            use gpui_component::WindowExt as _;
            assert_eq!(window.notifications(cx).len(), 1, "the result was shown");
        })
        .unwrap();
    }

    /// A capability's answer as JSON, for the ones this module's `ResultSet` does not name.
    fn raw(
        name: &str,
        params: serde_json::Value,
        cx: &mut gpui::App,
    ) -> Result<serde_json::Value, RequestError> {
        register(cx);
        let task =
            capability::dispatch(name, params, capability::Caller::Agent("pi".into()), cx).task;
        capability::now(task)
            .unwrap_or_else(|_| panic!("{name} did not answer at once"))
            .map(|answer| answer.value)
    }

    /// A patch stages several columns in one draft, skips a row whose guard no longer matches,
    /// and applies as one undo step.
    #[gpui::test]
    fn a_patch_stages_several_columns_and_guards_each_row(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            let staged = raw(
                "stage_patch",
                serde_json::json!({
                    "revision": revision(cx).0,
                    "rows": [
                        {"row": 0, "set": {"Title": "Harvest Festival", "Medium": "16mm film"},
                         "expect": {"Title": "Harvest"}},
                        {"row": 1, "set": {"Medium": "VHS"}, "expect": {"Title": "Not Wharf"}},
                        {"row": 2, "set": {"Medium": "Film"}}
                    ],
                    "note": "from the shot list"
                }),
                cx,
            )
            .unwrap();
            assert_eq!(staged["result"], "staged_patch");
            assert_eq!(staged["accepted"], 2);
            assert_eq!(staged["unchanged"], 1, "row 2 already says Film");
            assert_eq!(staged["stale"], serde_json::json!([1]));
            assert_eq!(cell(0, cx), "Harvest", "staging writes nothing");

            crate::agent::apply_all(&agent_source("pi"), cx);
            assert_eq!(cell(0, cx), "Harvest Festival");
            crate::history_step(false, cx);
            assert_eq!(
                cell(0, cx),
                "Harvest",
                "one undo takes back the whole patch"
            );
        });
    }

    #[gpui::test]
    fn variant_spellings_come_back_grouped_with_a_suggestion(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            raw(
                "add_rows",
                serde_json::json!({"revision": revision(cx).0, "rows": [{"Medium": "film"}]}),
                cx,
            )
            .unwrap();
            let groups = raw(
                "value_variants",
                serde_json::json!({"column": "Medium"}),
                cx,
            )
            .unwrap();
            assert_eq!(groups["groups"][0]["suggested"], "Film");
            assert_eq!(groups["groups"][0]["values"][0]["count"], 2);
            assert_eq!(groups["groups"][0]["values"][1]["value"], "film");
            assert_eq!(
                raw("value_variants", serde_json::json!({"column": "Nope"}), cx),
                Err(RequestError::UnknownColumn)
            );
        });
    }

    #[gpui::test]
    fn history_shows_an_agents_credited_change(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            raw(
                "add_column",
                serde_json::json!({"revision": revision(cx).0, "name": "Date"}),
                cx,
            )
            .unwrap();
            let page = raw("history", serde_json::json!({}), cx).unwrap();
            let newest = &page["entries"][0];
            assert_eq!(newest["credit"], "using pi");
            assert_eq!(newest["changes"][0], "added column Date");
        });
    }

    /// An agent's note is signed, replaceable by that agent, and never written over the archivist's.
    #[gpui::test]
    fn an_agent_signs_its_notes_and_leaves_others_alone(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            let note = |text: &str, cx: &mut gpui::App| {
                raw(
                    "add_note",
                    serde_json::json!({"revision": revision(cx).0, "row": 0, "column": "Title", "text": text}),
                    cx,
                )
            };
            note("Is this the 1974 festival?", cx).unwrap();
            let notes = raw("notes", serde_json::json!({"rows": [0]}), cx).unwrap();
            assert_eq!(notes["notes"][0]["text"], "pi: Is this the 1974 festival?");
            note("Probably 1975.", cx).unwrap();
            let notes = raw("notes", serde_json::json!({}), cx).unwrap();
            assert_eq!(notes["notes"].as_array().unwrap().len(), 1, "replaced, not added");

            let location = diagnostics::Location {
                dataset: DATASET_MAIN.into(),
                row: Some(1),
                row_id: Some(2),
                column: Some("Title".into()),
            };
            diagnostics::Diagnostics::set_note(
                location,
                "Check the reel label".into(),
                settings::history::Origin::Typed,
                cx,
            );
            let theirs = raw(
                "add_note",
                serde_json::json!({"revision": revision(cx).0, "row": 1, "column": "Title", "text": "x"}),
                cx,
            );
            assert!(matches!(theirs, Err(RequestError::EditRefused(_))));
        });
    }

    /// A relink points a row at a file that exists and is allowed, and applying repoints the row.
    #[gpui::test]
    fn a_staged_relink_repoints_the_row_once_applied(cx: &mut TestAppContext) {
        let dir = open_files("relink", cx);
        std::fs::create_dir_all(dir.join("photos/moved")).unwrap();
        std::fs::write(dir.join("photos/moved/1-final.jpg"), "1").unwrap();
        let outside = std::env::temp_dir().join("qrate-agent-relink-outside.jpg");
        std::fs::write(&outside, "x").unwrap();
        cx.update(|cx| {
            assert_eq!(
                raw(
                    "stage_relinks",
                    serde_json::json!({"revision": revision(cx).0, "links": [
                        {"row": 0, "path": outside.to_string_lossy()}
                    ]}),
                    cx,
                ),
                Err(RequestError::FileAccessRequired(
                    outside.to_string_lossy().into_owned()
                )),
                "a file outside the project needs a read grant"
            );
            let staged = raw(
                "stage_relinks",
                serde_json::json!({"revision": revision(cx).0, "links": [
                    {"row": 0, "path": "photos/moved/1-final.jpg"}
                ], "note": "moved by the digitisation vendor"}),
                cx,
            )
            .unwrap();
            assert_eq!(staged["accepted"], 1);
            assert_eq!(linked(0, cx).0, "1", "staging links nothing");
            crate::agent::apply_all(&agent_source("pi"), cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                linked(0, cx),
                ("moved/1-final.jpg".into(), Some("1-final.jpg".into()))
            );
            crate::history_step(false, cx);
            assert_eq!(linked(0, cx).0, "1");
        });
    }

    #[gpui::test]
    async fn candidates_for_a_file_are_found_by_name_and_stem(cx: &mut TestAppContext) {
        let dir = open_files("candidates", cx);
        std::fs::create_dir_all(dir.join("photos/elsewhere/deeper")).unwrap();
        std::fs::write(dir.join("photos/elsewhere/deeper/2.jpg"), "2").unwrap();
        std::fs::write(dir.join("photos/elsewhere/2.tif"), "2").unwrap();
        let task = cx.update(|cx| {
            register(cx);
            capability::dispatch(
                "find_candidates",
                serde_json::json!({"rows": [1], "folders": ["photos/elsewhere"]}),
                capability::Caller::Agent("pi".into()),
                cx,
            )
            .task
        });
        cx.run_until_parked();
        let found = task.await.unwrap().value;
        let candidates = found["items"][0]["candidates"].as_array().unwrap();
        assert_eq!(found["items"][0]["value"], "2");
        assert_eq!(found["items"][0]["missing"], false);
        let paths: Vec<&str> = candidates
            .iter()
            .map(|c| c["path"].as_str().unwrap())
            .collect();
        assert!(paths.iter().any(|path| path.ends_with("deeper/2.jpg")));
        assert!(paths.iter().any(|path| path.ends_with("elsewhere/2.tif")));
        assert!(
            candidates.iter().all(|c| c["reason"] == "same_name"),
            "the link names no extension, so either file matches it by name"
        );
    }

    /// A call that may answer later, as JSON.
    fn raw_later(
        name: &str,
        params: serde_json::Value,
        cx: &mut gpui::App,
    ) -> gpui::Task<Result<serde_json::Value, RequestError>> {
        register(cx);
        let task =
            capability::dispatch(name, params, capability::Caller::Agent("pi".into()), cx).task;
        cx.background_executor()
            .spawn(async move { task.await.map(|answer| answer.value) })
    }

    /// An adopted check asks first, then keeps publishing its findings as the table changes.
    #[gpui::test]
    async fn an_adopted_check_asks_then_runs_with_the_other_checks(cx: &mut TestAppContext) {
        open_in_window(cx);
        let asked = cx.update(|cx| {
            raw(
                "program_save",
                serde_json::json!({"source": "return function(q, args)\n  local out = {}\n  for i = 0, q:row_count() - 1 do\n    if q:cell(i, 'Medium') == 'Film' then table.insert(out, {row = i, column = 'Medium', message = 'Which gauge?'}) end\n  end\n  return out\nend"}),
                cx,
            )
            .unwrap();
            raw_later(
                "adopt_check",
                serde_json::json!({"revision": revision(cx).0, "name": "gauge", "reason": "Film rows need a gauge"}),
                cx,
            )
        });
        cx.run_until_parked();
        cx.update(|cx| crate::capabilities::answer_pending_confirm(true, cx));
        let adopted = asked.await.unwrap();
        assert_eq!(adopted["checks"][0]["name"], "gauge");
        assert_eq!(adopted["checks"][0]["agent"], "pi");
        cx.run_until_parked();
        cx.update(|cx| {
            let found: Vec<_> = diagnostics::Diagnostics::all(cx)
                .iter()
                .filter(|d| d.source.label() == "check: gauge")
                .map(|d| d.location.row)
                .collect();
            assert_eq!(
                found,
                [Some(0), Some(2)],
                "the program ran and its findings were published"
            );
            assert_eq!(
                raw("checks", serde_json::json!({}), cx).unwrap()["checks"][0]["name"],
                "gauge"
            );
        });
    }

    #[gpui::test]
    async fn an_export_check_reports_what_would_be_lost(cx: &mut TestAppContext) {
        open(cx);
        let (csv, xlsx, csl) = cx.update(|cx| {
            raw(
                "add_rows",
                serde_json::json!({"revision": revision(cx).0, "rows": [{"Title": "x".repeat(40_000)}, {"Medium": "Film"}]}),
                cx,
            )
            .unwrap();
            (
                raw_later("export_check", serde_json::json!({"format": "csv"}), cx),
                raw_later("export_check", serde_json::json!({"format": "xlsx"}), cx),
                raw_later("export_check", serde_json::json!({"format": "csl_json"}), cx),
            )
        });
        cx.run_until_parked();
        let csv = csv.await.unwrap();
        assert_eq!(csv["ok"], true);
        assert!(csv["bytes"].as_u64().unwrap() > 40_000);
        let xlsx = xlsx.await.unwrap();
        assert_eq!(xlsx["ok"], false);
        assert!(
            xlsx["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["row"] == 3 && p["severity"] == "fails"),
            "the over-long cell is named: {xlsx}"
        );
        let csl = csl.await.unwrap();
        assert!(
            csl["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["row"] == 4 && p["severity"] == "loses"),
            "the untitled row loses its title in Zotero: {csl}"
        );
    }

    /// A change set stages several kinds of change, asks before its deletion when applied, and
    /// undoes in one step.
    #[gpui::test]
    async fn a_change_set_applies_as_one_step_after_its_deletion_is_confirmed(
        cx: &mut TestAppContext,
    ) {
        open_in_window(cx);
        cx.update(|cx| {
            let staged = raw(
                "stage_changeset",
                serde_json::json!({
                    "revision": revision(cx).0,
                    "patch": [{"row": 0, "set": {"Title": "Harvest Festival"}}],
                    "add_rows": [{"Title": "Dock", "Medium": "Film"}],
                    "delete": {"rows": [2], "reason": "duplicate of row 1"},
                    "note": "merge the cannery reels"
                }),
                cx,
            )
            .unwrap();
            assert_eq!(staged["cells"], 1);
            assert_eq!(staged["adds"], 1);
            assert_eq!(staged["deletes"], 1);
            assert_eq!(cell(2, cx), "Cannery", "nothing changes while staged");
            crate::agent::apply_all(&agent_source("pi"), cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                cell(0, cx),
                "Harvest",
                "the deletion has not been confirmed yet"
            );
            crate::capabilities::answer_pending_confirm(true, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cell(0, cx), "Harvest Festival");
            assert_eq!(headers(cx).len(), 2);
            let titles: Vec<_> = (0..3).map(|row| cell(row, cx)).collect();
            assert_eq!(titles, ["Harvest Festival", "Wharf", "Dock"]);
            crate::history_step(false, cx);
            let titles: Vec<_> = (0..3).map(|row| cell(row, cx)).collect();
            assert_eq!(
                titles,
                ["Harvest", "Wharf", "Cannery"],
                "one undo takes it all back"
            );
        });
    }

    #[gpui::test]
    async fn following_along_hears_about_a_draft_being_applied(cx: &mut TestAppContext) {
        open(cx);
        let (waiting, seen) = cx.update(|cx| {
            let seen = revision(cx).0;
            let waiting = raw_later(
                "wait_for_change",
                serde_json::json!({"revision": seen, "selection": [], "timeout_s": 5}),
                cx,
            );
            (waiting, seen)
        });
        cx.update(|cx| {
            raw(
                "stage_column_rewrite",
                serde_json::json!({"revision": seen, "column": "Medium", "values": [[1, "VHS"]]}),
                cx,
            )
            .unwrap();
            crate::agent::apply_all(&agent_source("pi"), cx);
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        let changed = waiting.await.unwrap();
        assert_eq!(changed["changed"], true);
        assert_eq!(changed["events"][0]["kind"], "draft_applied");
        assert_eq!(changed["events"][0]["agent"], "pi");
    }

    /// Retyping a file column would leave rows naming files that do not exist while nothing on
    /// disk moves, so every draft that tries is refused and pointed at renames or relinks.
    #[gpui::test]
    fn a_file_column_is_renamed_or_relinked_never_retyped(cx: &mut TestAppContext) {
        let dir = open_files("retype", cx);
        cx.update(|cx| {
            let at = revision(cx).0;
            let refused = RequestError::FileColumn("File".into());
            assert_eq!(
                raw(
                    "stage_column_rewrite",
                    serde_json::json!({"revision": at, "column": "File", "values": [[0, "AD-001"]]}),
                    cx
                ),
                Err(refused.clone())
            );
            assert_eq!(
                raw(
                    "stage_patch",
                    serde_json::json!({"revision": at, "rows": [{"row": 0, "set": {"File": "AD-001", "Title": "x"}}]}),
                    cx
                ),
                Err(refused.clone())
            );
            assert_eq!(
                raw(
                    "stage_findings",
                    serde_json::json!({"revision": at, "findings": [{"row": 0, "column": "File", "severity": "warning",
                        "message": "Rename to the new id", "expected": "1", "replacement": "AD-001"}]}),
                    cx
                ),
                Err(refused)
            );
            // A finding without a replacement, and a patch of other columns, are fine.
            assert!(
                raw(
                    "stage_findings",
                    serde_json::json!({"revision": at, "findings": [{"row": 0, "column": "File", "severity": "note",
                        "message": "Old naming scheme", "expected": "1"}]}),
                    cx
                )
                .is_ok()
            );
            assert_eq!(linked(0, cx), ("1".into(), Some("1.jpg".into())));
        });
        assert!(dir.join("photos/1.jpg").exists());
    }

    /// An agent proposing more changes adds to what it proposed, rather than wiping it.
    #[gpui::test]
    fn a_second_patch_adds_to_the_first(cx: &mut TestAppContext) {
        open(cx);
        cx.update(|cx| {
            let at = revision(cx).0;
            let patch = |row: usize, value: &str, cx: &mut gpui::App| {
                raw(
                    "stage_patch",
                    serde_json::json!({"revision": at, "rows": [{"row": row, "set": {"Medium": value}}]}),
                    cx,
                )
                .unwrap()
            };
            patch(0, "16mm", cx);
            patch(1, "VHS-C", cx);
            patch(0, "35mm", cx);
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            let after = |row: usize, cx: &gpui::App| {
                crate::agent::staged_change(state.read(cx).delegate(), row, 1, cx)
                    .map(|change| change.after.to_string())
            };
            assert_eq!(after(0, cx).as_deref(), Some("35mm"), "a row staged again takes the newer value");
            assert_eq!(after(1, cx).as_deref(), Some("VHS-C"), "the earlier row is still staged");
            crate::agent::apply_all(&agent_source("pi"), cx);
            assert_eq!(cell_of(0, "Medium", cx), "35mm");
            assert_eq!(cell_of(1, "Medium", cx), "VHS-C");
        });
    }

    fn cell_of(row: usize, column: &str, cx: &mut gpui::App) -> String {
        let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
        let delegate = state.read(cx).delegate();
        delegate
            .cell(row, delegate.data_col(column).unwrap())
            .unwrap()
            .to_string()
    }

    /// Restoring to the start of History gives back the project as it was before any recorded
    /// change: the History panel's "Start of history" row.
    #[gpui::test]
    fn restoring_to_the_start_of_history_gives_back_the_loaded_data(cx: &mut TestAppContext) {
        let file = std::env::temp_dir().join("qrate-agent-restore-start.qrate");
        let _ = std::fs::remove_file(&file);
        let mut project = project();
        project.file = file.clone();
        settings::project::create_project_file(
            &file,
            &settings::project::ProjectSpec {
                name: &project.data.name,
                source: "CSV",
                columns: &project.data.columns,
                headers: &project.data.headers,
                rows: &project.data.rows,
                ..Default::default()
            },
        )
        .unwrap();
        cx.update(|cx| {
            register(cx);
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project);
        });
        cx.add_window_view(TablePanel::new);
        cx.run_until_parked();
        cx.update(|cx| {
            crate::write_cells(
                vec![(0, 0, "Harvest Festival".into())],
                settings::history::Origin::Typed,
                cx,
            );
            raw(
                "add_rows",
                serde_json::json!({"revision": revision(cx).0, "rows": [{"Title": "Dock"}]}),
                cx,
            )
            .unwrap();
            assert_eq!(cell(0, cx), "Harvest Festival");
            crate::restore_to(0, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let titles: Vec<_> = (0..3).map(|row| cell(row, cx)).collect();
            assert_eq!(titles, ["Harvest", "Wharf", "Cannery"]);
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            assert_eq!(
                state.read(cx).delegate().row_count(),
                3,
                "the added row is gone"
            );
        });
    }
}
