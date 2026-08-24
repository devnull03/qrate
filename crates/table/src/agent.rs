//! Live-table adapter for the external-agent contract: the reads, and the one draft-only write.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use ai::agent::{
    Column, Count, DiagnosticSummary, FilterOp, Finding, MAX_RESULT_BYTES, Overview, ProgramAudit,
    ProgramOutput, ProjectSummary, Query, QueryResult, QuerySource, Request, RequestError,
    Response, ResultSet, Severity, TableRevision,
};
use base64::Engine as _;
use diagnostics::{DATASET_MAIN, FixProviders, Location, Source};
use gpui::{App, Global, SharedString, Task};
use sha2::{Digest as _, Sha256};

use crate::{QrateTableDelegate, TableStateHandle};

/// The diagnostics source every staged finding is filed under. One name for the whole agent, so a
/// re-run replaces its own drafts wholesale instead of stacking a second copy beside them.
pub const AGENT_SOURCE: &str = "agent";

#[derive(Default)]
struct AgentProgram {
    source: String,
    version: u64,
    hash: String,
}
impl Global for AgentProgram {}
static PROGRAM_RUNS: AtomicU64 = AtomicU64::new(1);

/// Answer one validated agent request from qrate's live table state.
///
/// This deliberately reads the delegate rather than the `.qrate` database, so responses include
/// unsaved cell edits and the current table generation. Nothing here writes a cell: the one
/// non-read, [`Request::StageFindings`], publishes advisory diagnostics that are recomputed rather
/// than persisted, and offers corrections the archivist still has to click.
pub fn respond_to_agent(request: Request, cx: &mut App) -> Result<Response, RequestError> {
    request.validate()?;

    match request {
        Request::StageFindings { revision, findings } => {
            return stage_findings(revision, findings, cx);
        }
        Request::ProgramSave { source } => return save_program(source, cx),
        _ => {}
    }

    let project = cx
        .try_global::<settings::project::CurrentProject>()
        .ok_or(RequestError::ProjectUnavailable)?;
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let table = state.read(cx);
    let delegate = table.delegate();
    let revision = TableRevision(delegate.values_generation());

    let result = match request {
        Request::Overview => ResultSet::Overview(overview(project, delegate, cx)),
        Request::Query(query) => ResultSet::Query(run_query(query, revision, delegate, cx)?),
        Request::ProgramRun {
            revision: judged,
            args,
        } => {
            if judged != revision {
                return Err(RequestError::StaleCursor);
            }
            let program = cx
                .try_global::<AgentProgram>()
                .ok_or(RequestError::ProgramUnavailable)?;
            let (version, hash, source) = (
                program.version,
                program.hash.clone(),
                program.source.clone(),
            );
            let snapshot = program_snapshot(revision, delegate, cx);
            let output =
                plugin_host::run_agent_program(&source, &args, snapshot).map_err(|error| {
                    log::warn!("agent scratch program {hash} failed: {error}");
                    if error.kind == "output_too_large" {
                        RequestError::ProgramOutputTooLarge(error.detail)
                    } else {
                        RequestError::ProgramFailed(error.detail)
                    }
                })?;
            let run_id = format!(
                "{}-{}-{}",
                &hash[..12],
                revision.0,
                PROGRAM_RUNS.fetch_add(1, Ordering::Relaxed)
            );
            let audit = ProgramAudit {
                run_id,
                revision,
                version,
                hash: hash.clone(),
                elapsed_ms: output.elapsed_ms,
                memory_limit_bytes: plugin_host::MEMORY_LIMIT,
                deadline_ms: plugin_host::DEADLINE.as_millis() as u64,
                output_limit_bytes: plugin_host::MAX_OUTPUT,
                status: "success".into(),
                output_bytes: output.output_bytes,
            };
            log::info!(
                "agent program run {} revision {} version {} sha256 {} status success elapsed_ms {} output_bytes {}",
                audit.run_id,
                revision.0,
                version,
                hash,
                audit.elapsed_ms,
                audit.output_bytes
            );
            ResultSet::ProgramRun(ProgramOutput {
                version,
                hash,
                value: output.value,
                logs: output.logs,
                elapsed_ms: output.elapsed_ms,
                audit,
            })
        }
        Request::Thumbnails { items } => {
            let mut thumbnails = Vec::with_capacity(items.len());
            for item in items {
                let path = delegate
                    .row_image(item.row)
                    .ok_or(RequestError::ThumbnailUnavailable)?;
                let bytes = preview::thumbnail_png(path, item.page)
                    .ok_or(RequestError::ThumbnailUnavailable)?;
                thumbnails.push(ai::agent::Thumbnail {
                    row: item.row,
                    page: item.page,
                    media_type: "image/png".into(),
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
            ResultSet::Thumbnails { items: thumbnails }
        }
        Request::ProgramSave { .. } | Request::StageFindings { .. } => unreachable!(),
    };

    Ok(Response { revision, result })
}

/// Capture live GPUI-owned state now, then do bounded analysis away from the UI thread.
pub fn respond_to_agent_async(
    request: Request,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    if let Err(error) = request.validate() {
        return Task::ready(Err(error));
    }
    match request {
        Request::ProgramSave { source } => {
            let Some(state) = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
            else {
                return Task::ready(Err(RequestError::TableUnavailable));
            };
            let revision = TableRevision(state.read(cx).delegate().values_generation());
            let version = cx
                .try_global::<AgentProgram>()
                .map_or(1, |program| program.version + 1);
            let background = cx.background_executor().spawn(async move {
                plugin_host::validate_agent_program(&source)
                    .map_err(|error| RequestError::ProgramCompileFailed(error.detail))?;
                let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
                Ok::<_, RequestError>((source, hash))
            });
            cx.spawn(async move |cx| {
                let (source, hash) = background.await?;
                cx.update(|cx| {
                    cx.set_global(AgentProgram {
                        source,
                        version,
                        hash: hash.clone(),
                    })
                });
                Ok(Response {
                    revision,
                    result: ResultSet::ProgramSaved { version, hash },
                })
            })
        }
        Request::Query(query) => {
            let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
                return Task::ready(Err(RequestError::ProjectUnavailable));
            };
            let Some(state) = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
            else {
                return Task::ready(Err(RequestError::TableUnavailable));
            };
            let state = state.read(cx);
            let delegate = state.delegate();
            let revision = TableRevision(delegate.values_generation());
            let diagnostic_source = matches!(query.source, QuerySource::Diagnostics { .. });
            let schema = if diagnostic_source {
                ["row", "column", "severity", "source", "message"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            } else {
                std::iter::once("row".into())
                    .chain(project.data.headers.iter().cloned())
                    .collect()
            };
            let records = match &query.source {
                QuerySource::Diagnostics {
                    severities,
                    sources,
                } => diagnostic_records(cx, severities, sources),
                source => row_records(delegate, source),
            };
            cx.background_executor().spawn(async move {
                run_query_records(query, revision, records, schema, diagnostic_source).map(
                    |result| Response {
                        revision,
                        result: ResultSet::Query(result),
                    },
                )
            })
        }
        Request::ProgramRun {
            revision: judged,
            args,
        } => {
            let Some(state) = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
            else {
                return Task::ready(Err(RequestError::TableUnavailable));
            };
            let state = state.read(cx);
            let delegate = state.delegate();
            let revision = TableRevision(delegate.values_generation());
            if judged != revision {
                return Task::ready(Err(RequestError::StaleCursor));
            }
            let Some(program) = cx.try_global::<AgentProgram>() else {
                return Task::ready(Err(RequestError::ProgramUnavailable));
            };
            let (version, hash, source) = (
                program.version,
                program.hash.clone(),
                program.source.clone(),
            );
            let snapshot = program_snapshot(revision, delegate, cx);
            cx.background_executor().spawn(async move {
                let run_id = format!(
                    "{}-{}-{}",
                    &hash[..12],
                    revision.0,
                    PROGRAM_RUNS.fetch_add(1, Ordering::Relaxed)
                );
                let output = match plugin_host::run_agent_program(&source, &args, snapshot) {
                    Ok(output) => output,
                    Err(error) => {
                        log::warn!(
                            "agent program run {} revision {} version {} sha256 {} status {} elapsed_ms {} output_bytes 0 memory_limit_bytes {} deadline_ms {} output_limit_bytes {}",
                            run_id, revision.0, version, hash, error.kind, error.elapsed_ms,
                            plugin_host::MEMORY_LIMIT, plugin_host::DEADLINE.as_millis(), plugin_host::MAX_OUTPUT
                        );
                        return Err(if error.kind == "output_too_large" {
                            RequestError::ProgramOutputTooLarge(error.detail)
                        } else {
                            RequestError::ProgramFailed(error.detail)
                        });
                    }
                };
                let audit = ProgramAudit {
                    run_id,
                    revision,
                    version,
                    hash: hash.clone(),
                    elapsed_ms: output.elapsed_ms,
                    memory_limit_bytes: plugin_host::MEMORY_LIMIT,
                    deadline_ms: plugin_host::DEADLINE.as_millis() as u64,
                    output_limit_bytes: plugin_host::MAX_OUTPUT,
                    status: "success".into(),
                    output_bytes: output.output_bytes,
                };
                log::info!(
                    "agent program run {} revision {} version {} sha256 {} status success elapsed_ms {} output_bytes {} memory_limit_bytes {} deadline_ms {} output_limit_bytes {}",
                    audit.run_id, revision.0, version, hash, audit.elapsed_ms, audit.output_bytes,
                    audit.memory_limit_bytes, audit.deadline_ms, audit.output_limit_bytes
                );
                Ok(Response {
                    revision,
                    result: ResultSet::ProgramRun(ProgramOutput {
                        version,
                        hash,
                        value: output.value,
                        logs: output.logs,
                        elapsed_ms: output.elapsed_ms,
                        audit,
                    }),
                })
            })
        }
        Request::Thumbnails { items } => {
            let Some(state) = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
            else {
                return Task::ready(Err(RequestError::TableUnavailable));
            };
            let state = state.read(cx);
            let delegate = state.delegate();
            let revision = TableRevision(delegate.values_generation());
            let paths: Result<Vec<_>, _> = items
                .into_iter()
                .map(|item| {
                    delegate
                        .row_image(item.row)
                        .map(|path| (item, path.to_path_buf()))
                        .ok_or(RequestError::ThumbnailUnavailable)
                })
                .collect();
            let Ok(paths) = paths else {
                return Task::ready(Err(RequestError::ThumbnailUnavailable));
            };
            cx.background_executor().spawn(async move {
                let mut thumbnails = Vec::with_capacity(paths.len());
                for (item, path) in paths {
                    let bytes = preview::thumbnail_png(&path, item.page)
                        .ok_or(RequestError::ThumbnailUnavailable)?;
                    thumbnails.push(ai::agent::Thumbnail {
                        row: item.row,
                        page: item.page,
                        media_type: "image/png".into(),
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                    });
                }
                Ok(Response {
                    revision,
                    result: ResultSet::Thumbnails { items: thumbnails },
                })
            })
        }
        other => Task::ready(respond_to_agent(other, cx)),
    }
}

fn program_snapshot(
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

fn save_program(source: String, cx: &mut App) -> Result<Response, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let revision = TableRevision(state.read(cx).delegate().values_generation());
    plugin_host::validate_agent_program(&source).map_err(|error| {
        log::warn!("agent scratch program was not activated: {error}");
        RequestError::ProgramCompileFailed(error.detail)
    })?;
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let version = cx.try_global::<AgentProgram>().map_or(1, |p| p.version + 1);
    cx.set_global(AgentProgram {
        source,
        version,
        hash: hash.clone(),
    });
    Ok(Response {
        revision,
        result: ResultSet::ProgramSaved { version, hash },
    })
}

fn overview(
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

fn run_query(
    query: Query,
    revision: TableRevision,
    delegate: &QrateTableDelegate,
    cx: &App,
) -> Result<QueryResult, RequestError> {
    let diagnostic_source = matches!(query.source, QuerySource::Diagnostics { .. });
    let source_schema: Vec<String> = if diagnostic_source {
        ["row", "column", "severity", "source", "message"]
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        std::iter::once("row".into())
            .chain(delegate.dataset_snapshot().0)
            .collect()
    };
    let records = match &query.source {
        QuerySource::Diagnostics {
            severities,
            sources,
        } => diagnostic_records(cx, severities, sources),
        source => row_records(delegate, source),
    };
    run_query_records(query, revision, records, source_schema, diagnostic_source)
}

fn run_query_records(
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

fn row_records(delegate: &QrateTableDelegate, source: &QuerySource) -> Vec<Record> {
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

fn diagnostic_records(cx: &App, severities: &[Severity], sources: &[String]) -> Vec<Record> {
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

/// Every replacement the agent staged, by `(row, column)`.
///
/// A global rather than a field on anything, because [`FixProviders`] registers a plain `fn`
/// pointer and the offer has no handle to reach back through. Never persisted, and replaced
/// wholesale on the next batch — the same replace-by-source rule the diagnostics follow.
#[derive(Default)]
struct StagedFixes {
    /// The table the whole batch was judged against, retained per the contract. Read by the apply
    /// path, which must refuse a batch whose table has moved on under it.
    #[allow(dead_code)]
    revision: u64,
    by_cell: HashMap<(usize, SharedString), Vec<StagedFix>>,
}
impl Global for StagedFixes {}

/// Publish a batch of drafts: findings into the Problems panel, replacements into the Fixes menu.
/// No cell is touched, and a draft the table has moved past is dropped rather than shown.
fn stage_findings(
    revision: TableRevision,
    findings: Vec<Finding>,
    cx: &mut App,
) -> Result<Response, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;

    let mut published = Vec::new();
    let mut fixes: HashMap<(usize, SharedString), Vec<StagedFix>> = HashMap::new();
    let mut stale = Vec::new();

    let table = state.read(cx);
    let delegate = table.delegate();
    let current = TableRevision(delegate.values_generation());

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
            source: Source::Validator(AGENT_SOURCE.into()),
            message: finding.message.into(),
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
    diagnostics::Diagnostics::set(
        &Source::Validator(AGENT_SOURCE.into()),
        DATASET_MAIN,
        published,
        cx,
    );
    cx.set_global(StagedFixes {
        revision: revision.0,
        by_cell: fixes,
    });
    FixProviders::register(AGENT_SOURCE, offer_staged, cx);
    log::info!(
        "agent staged {accepted} finding(s) against revision {}, dropping {} judged against text \
         the table has moved past (current revision {})",
        revision.0,
        stale.len(),
        current.0
    );

    Ok(Response {
        revision: current,
        result: ResultSet::Staged { accepted, stale },
    })
}

/// What the Fixes menu offers for a staged finding — but only while the cell still says what the
/// agent judged, because a cell edited since staging is a cell nobody reviewed.
fn offer_staged(location: &Location, text: &str, cx: &App) -> Vec<diagnostics::Fix> {
    let (Some(row), Some(column)) = (location.row, location.column.clone()) else {
        return Vec::new();
    };
    cx.try_global::<StagedFixes>()
        .and_then(|staged| staged.by_cell.get(&(row, column)))
        .map(|staged| {
            staged
                .iter()
                .filter(|fix| fix.expected == text)
                .map(|fix| diagnostics::Fix {
                    label: format!("Use “{}”", fix.replacement).into(),
                    replacement: fix.replacement.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note on `note.rs`'s test module.
    use ai::agent::{
        Finding, Overview, ProjectSummary, Query, QuerySource, Request, RequestError, ResultSet,
        TableRevision,
    };
    use diagnostics::{DATASET_MAIN, Location};
    use gpui::TestAppContext;

    use crate::{AGENT_SOURCE, TablePanel, TableStateHandle, respond_to_agent};

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

    /// The end of the bridge an agent actually grades its review on: real cell values out of the
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
            assert_eq!(published[0].source.label(), AGENT_SOURCE);
            assert_eq!(published[0].severity, diagnostics::Severity::Warning);

            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(1),
                column: Some("Title".into()),
            };
            let offered = diagnostics::fixes::at(&location, "Harvest", cx);
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
                diagnostics::fixes::at(&location, "Harvest, retitled by hand", cx).is_empty(),
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
                .filter(|d| d.source.label() == AGENT_SOURCE)
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
}
