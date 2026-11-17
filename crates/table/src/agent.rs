//! Live-table adapter for the external-agent contract: the reads, and the one draft-only write.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ai::agent::{
    Column, Count, DiagnosticSummary, FilterOp, Finding, MAX_RESULT_BYTES, Overview, ProgramAudit,
    ProgramOutput, ProjectSummary, Query, QueryResult, QuerySource, Rename, Request, RequestError,
    Response, ResultSet, RowPlacement, Severity, TableEdit, TableRevision,
};
use base64::Engine as _;
use diagnostics::fixes::{GroupFix, SourceActions};
use diagnostics::{DATASET_MAIN, FixProviders, Location, Source};
use gpui::{
    AnyElement, App, Empty, Global, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, Task, Window, div,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::notification::Notification;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use settings::columns::ColumnType;
use settings::history::Origin;
use sha2::{Digest as _, Sha256};

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
pub fn respond_to_agent(
    request: Request,
    agent: &str,
    cx: &mut App,
) -> Result<Response, RequestError> {
    request.validate()?;

    match request {
        Request::StageFindings { revision, findings } => {
            return stage_findings(agent, revision, findings, cx);
        }
        Request::StageColumnRewrite {
            revision,
            column,
            values,
            note,
        } => return stage_column_rewrite(agent, revision, column, values, note, cx),
        Request::StageRenames { revision, renames } => {
            return stage_renames(agent, revision, renames, cx);
        }
        Request::Edit { revision, edit } => return edit_table(agent, revision, edit, cx),
        // A delete has to wait for the archivist, which only the asynchronous path can do.
        Request::Delete { .. }
        | Request::RequestAccess { .. }
        | Request::CheckAccess { .. }
        | Request::ForgetAccess {}
        | Request::ImportFiles { .. } => return Err(RequestError::NoWindow),
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
        Request::ProgramSave { .. }
        | Request::StageFindings { .. }
        | Request::StageColumnRewrite { .. }
        | Request::StageRenames { .. }
        | Request::Edit { .. }
        | Request::Delete { .. }
        | Request::RequestAccess { .. }
        | Request::CheckAccess { .. }
        | Request::ForgetAccess {}
        | Request::ImportFiles { .. } => unreachable!(),
    };

    Ok(Response { revision, result })
}

/// Capture live GPUI-owned state now, then do bounded analysis away from the UI thread.
pub fn respond_to_agent_async(
    request: Request,
    agent: &str,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    if let Err(error) = request.validate() {
        return Task::ready(Err(error));
    }
    match request {
        Request::RequestAccess { path, reason } => {
            crate::agent_files::request_access(agent, path, reason, cx)
        }
        Request::CheckAccess { path } => crate::agent_files::check_access(agent, path, cx),
        Request::ForgetAccess {} => Task::ready(crate::agent_files::forget_access(agent, cx)),
        Request::ImportFiles {
            revision,
            paths,
            recursive,
        } => crate::agent_files::import_files(agent, revision, paths, recursive, cx),

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
        Request::Delete {
            revision,
            rows,
            column,
            reason,
        } => ask_to_delete(agent, revision, rows, column, reason, cx),
        other => Task::ready(respond_to_agent(other, agent, cx)),
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

/// One column's staged new values: source row → (text when staged, new text).
struct Rewrite {
    note: Option<String>,
    rows: BTreeMap<usize, (SharedString, SharedString)>,
}

impl Rewrite {
    fn title(&self, column: &str) -> String {
        let note = self.note.as_ref().map(|note| format!(" ({note})"));
        format!(
            "Rewrite {column}: {} rows{}",
            self.rows.len(),
            note.unwrap_or_default()
        )
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
fn stage_findings(
    agent: &str,
    revision: TableRevision,
    findings: Vec<Finding>,
    cx: &mut App,
) -> Result<Response, RequestError> {
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

    Ok(Response {
        revision: current,
        result: ResultSet::Staged { accepted, stale },
    })
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
    if staged.0.get(source).is_some_and(|batch| {
        batch.findings.is_empty() && batch.rewrites.is_empty() && batch.renames.is_empty()
    }) {
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
        batch
            .findings
            .iter()
            .cloned()
            .chain(rewrites)
            .chain(renames)
            .collect()
    });
    diagnostics::Diagnostics::set(&Source::Validator(source.clone()), DATASET_MAIN, items, cx);
    FixProviders::register(source, offer_staged, cx);
    SourceActions::register(source, staged_actions, cx);
    // The grid marks staged cells and the views show the staged bar; both have to redraw.
    cx.refresh_windows();
}

/// Stage one column's new values as a single draft, replacing this agent's earlier rewrite of the
/// same column. An empty `values` retracts it. No cell is touched.
fn stage_column_rewrite(
    agent: &str,
    revision: TableRevision,
    column: String,
    values: Vec<(usize, String)>,
    note: Option<String>,
    cx: &mut App,
) -> Result<Response, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let current = TableRevision(delegate.values_generation());
    // A rewrite quotes no cell text, so the revision is its only proof of what the agent read.
    if revision != current {
        return Err(RequestError::StaleRevision);
    }
    let col = delegate
        .data_col(&column)
        .ok_or(RequestError::UnknownColumn)?;
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
        batch
            .rewrites
            .insert(column.clone().into(), Rewrite { note, rows });
    }
    publish(&source, cx);
    log::info!(
        "{agent} staged a rewrite of {accepted} cell(s) in {column} at revision {}, {} already \
         holding the value",
        current.0,
        unchanged.len()
    );

    Ok(Response {
        revision: current,
        result: ResultSet::StagedRewrite {
            accepted,
            unchanged,
        },
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
        fix: format!("Rewrite {column}"),
    };
    crate::write_cells(cells, origin, cx);
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
fn stage_renames(
    agent: &str,
    revision: TableRevision,
    renames: Vec<Rename>,
    cx: &mut App,
) -> Result<Response, RequestError> {
    let root = project_dir(cx)?;
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let delegate = state.read(cx).delegate();
    let current = TableRevision(delegate.values_generation());
    if revision != current {
        return Err(RequestError::StaleRevision);
    }

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
    Ok(Response {
        revision: current,
        result: ResultSet::StagedRenames { accepted },
    })
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
fn in_active_window(cx: &mut App, show: impl FnOnce(&mut Window, &mut App) + 'static) {
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
        crate::write_cells(vec![(row, col, change.after)], origin, cx);
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
}

/// Apply everything one agent staged: its cell values as one undo step, then its file renames as
/// another. Cells and files changed since the agent looked are skipped.
pub(crate) fn apply_all(source: &SharedString, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) else {
        return;
    };
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
    for (column, rewrite) in &rewrites {
        let Some(col) = delegate.data_col(column) else {
            continue;
        };
        for (row, (before, after)) in &rewrite.rows {
            if delegate.cell(*row, col) == Some(before) {
                cells.insert((*row, col), after.clone());
            }
        }
    }
    let (applied, stale) = (cells.len(), total - cells.len());
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
        crate::write_cells(cells, origin, cx);
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
    log::info!("{agent}: {message}");
    tell(message, cx);
}

/// Drop everything one agent staged, touching no cell or file.
pub(crate) fn discard_all(source: &SharedString, cx: &mut App) {
    if let Some(batch) = cx.default_global::<StagedFixes>().0.get_mut(source) {
        *batch = Batch::default();
    }
    publish(source, cx);
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
fn edit_table(
    agent: &str,
    revision: TableRevision,
    edit: TableEdit,
    cx: &mut App,
) -> Result<Response, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    // Rows are named by index, so an edit read against an older table could land on another row.
    if revision != TableRevision(state.read(cx).delegate().values_generation()) {
        return Err(RequestError::StaleRevision);
    }
    let column = |name: &str, cx: &App| {
        state
            .read(cx)
            .delegate()
            .data_col(name)
            .ok_or(RequestError::UnknownColumn)
    };
    let row = |row: usize, cx: &App| {
        (row < state.read(cx).delegate().row_count())
            .then_some(row)
            .ok_or(RequestError::RowOutOfRange)
    };
    let new_name = |name: &str, cx: &App| {
        let name = name.trim();
        if name.is_empty() || state.read(cx).delegate().data_col(name).is_some() {
            return Err(RequestError::ColumnNameTaken(name.to_owned()));
        }
        Ok(SharedString::from(name.to_owned()))
    };
    let refused = |what: &str| RequestError::EditRefused(what.to_owned());

    let origin = Origin::Agent {
        agent: agent.to_owned(),
        fix: match &edit {
            TableEdit::AddRows { rows } => match rows.len() {
                1 => "Add a row".into(),
                n => format!("Add {n} rows"),
            },
            TableEdit::AddColumn { name, .. } => format!("Add column {}", name.trim()),
            TableEdit::RenameColumn { column, name } => {
                format!("Rename column {column} to {}", name.trim())
            }
            TableEdit::MoveColumn { column, .. } => format!("Move column {column}"),
            TableEdit::DescribeColumn { column, .. } => format!("Describe column {column}"),
            TableEdit::MoveRow { row, .. } => format!("Move row {}", row + 1),
            TableEdit::IndentRow { row } => format!("Indent row {}", row + 1),
            TableEdit::OutdentRow { row } => format!("Outdent row {}", row + 1),
            TableEdit::DuplicateRow { row } => format!("Duplicate row {}", row + 1),
            TableEdit::Filter { column, keep } => match keep {
                Some(_) => format!("Filter {column}"),
                None => format!("Clear the filter on {column}"),
            },
        },
    };
    let (detail, rows): (String, Vec<usize>) = match edit {
        TableEdit::AddRows { rows } => {
            let delegate = state.read(cx).delegate();
            let width = delegate.column_count();
            let mut values = Vec::with_capacity(rows.len());
            for fields in &rows {
                let mut cells = vec![SharedString::default(); width];
                for (name, value) in fields {
                    cells[column(name, cx)?] = value.clone().into();
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
            diagnostics::Diagnostics::align_note_rows(DATASET_MAIN, &row_ids, origin.clone(), cx);
            settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
            crate::revalidate_now(cx);
            crate::autosave(cx);
            let detail = match added {
                1 => "Add a row".to_owned(),
                n => format!("Add {n} rows"),
            };
            (detail, (at..at + added).collect())
        }
        TableEdit::AddColumn { name, after } => {
            let name = new_name(&name, cx)?;
            let at = match after {
                Some(after) => column(&after, cx)? + 1,
                None => 0,
            };
            crate::structural(
                crate::Structural::AddNamedColumn {
                    at,
                    name: name.clone(),
                },
                origin.clone(),
                cx,
            );
            (format!("Add column {name}"), Vec::new())
        }
        TableEdit::RenameColumn { column: from, name } => {
            let col = column(&from, cx)?;
            let name = new_name(&name, cx)?;
            crate::structural(
                crate::Structural::RenameColumn {
                    col,
                    name: name.clone(),
                },
                origin.clone(),
                cx,
            );
            (format!("Rename column {from} to {name}"), Vec::new())
        }
        TableEdit::MoveColumn {
            column: name,
            after,
        } => {
            let from = column(&name, cx)?;
            let to = match after {
                Some(after) => {
                    let after = column(&after, cx)?;
                    if after < from { after + 1 } else { after }
                }
                None => 0,
            };
            crate::structural(
                crate::Structural::MoveColumn { from, to },
                origin.clone(),
                cx,
            );
            (format!("Move column {name}"), Vec::new())
        }
        TableEdit::DescribeColumn {
            column: name,
            data_type,
            description,
        } => {
            column(&name, cx)?;
            let kind = match data_type {
                Some(data_type) => Some(
                    settings::columns::ColumnType::ALL
                        .into_iter()
                        .find(|kind| kind.as_str().eq_ignore_ascii_case(data_type.trim()))
                        .ok_or(RequestError::UnknownDataType(data_type))?,
                ),
                None => None,
            };
            let descriptions =
                |cx: &App| -> BTreeMap<String, settings::history::ColumnDescription> {
                    cx.global::<settings::project::CurrentProject>()
                        .data
                        .columns
                        .iter()
                        .map(|column| {
                            (
                                column.name.clone(),
                                settings::history::ColumnDescription {
                                    data_type: column.data_type.clone(),
                                    notes: column.notes.clone(),
                                },
                            )
                        })
                        .collect()
                };
            let before = descriptions(cx);
            if let Some(kind) = kind {
                settings::project::CurrentProject::set_column_type(&name, kind.as_str(), cx);
            }
            if let Some(description) = &description {
                settings::project::CurrentProject::set_column_notes(&name, description.trim(), cx);
            }
            let changes = descriptions(cx)
                .into_iter()
                .filter_map(|(column, after)| {
                    let before = before.get(&column).cloned().unwrap_or_default();
                    (before != after).then_some(settings::history::Change::ColumnDescribed {
                        column,
                        before,
                        after,
                    })
                })
                .collect();
            state.update(cx, |state, cx| {
                state
                    .delegate_mut()
                    .record_descriptions(changes, origin.clone());
                cx.emit(crate::TableChanged);
                cx.notify();
            });
            settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
            crate::autosave(cx);
            crate::revalidate_now(cx);
            (format!("Describe column {name}"), Vec::new())
        }
        TableEdit::MoveRow {
            row: moved,
            target,
            placement,
        } => {
            let (moved, target) = (row(moved, cx)?, row(target, cx)?);
            let placement = match placement {
                RowPlacement::Before => crate::RowPlacement::Before,
                RowPlacement::After => crate::RowPlacement::After,
                RowPlacement::Child => crate::RowPlacement::Child,
            };
            let op = crate::Arrangement::Move {
                row: moved,
                target,
                placement,
            };
            if !crate::arrange(op, origin.clone(), cx) {
                return Err(refused("the row cannot go there"));
            }
            (format!("Move row {}", moved + 1), Vec::new())
        }
        TableEdit::IndentRow { row: indented } => {
            let indented = row(indented, cx)?;
            if !crate::arrange(crate::Arrangement::Indent(indented), origin.clone(), cx) {
                return Err(refused("the row has no row above it at its level"));
            }
            (format!("Indent row {}", indented + 1), Vec::new())
        }
        TableEdit::OutdentRow { row: outdented } => {
            let outdented = row(outdented, cx)?;
            if !crate::arrange(crate::Arrangement::Outdent(outdented), origin.clone(), cx) {
                return Err(refused("the row is already at the top level"));
            }
            (format!("Outdent row {}", outdented + 1), Vec::new())
        }
        TableEdit::DuplicateRow { row: copied } => {
            let copied = row(copied, cx)?;
            crate::structural(
                crate::Structural::DuplicateRow { row: copied },
                origin.clone(),
                cx,
            );
            (format!("Duplicate row {}", copied + 1), vec![copied + 1])
        }
        TableEdit::Filter { column: name, keep } => {
            let col = column(&name, cx)?;
            state.update(cx, |state, cx| {
                let delegate = state.delegate_mut();
                let keep: Option<Vec<SharedString>> = keep
                    .as_ref()
                    .map(|values| values.iter().cloned().map(Into::into).collect());
                delegate.filter_from_agent(col, keep.as_deref(), origin.clone());
                state.refresh(cx);
                cx.emit(crate::TableChanged);
                cx.notify();
            });
            settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
            crate::autosave(cx);
            let detail = match keep {
                Some(_) => format!("Filter {name}"),
                None => format!("Clear the filter on {name}"),
            };
            (detail, Vec::new())
        }
    };

    log::info!("{agent}: {detail}");
    Ok(Response {
        revision: TableRevision(state.read(cx).delegate().values_generation()),
        result: ResultSet::Edited { detail, rows },
    })
}

/// How long a delete waits for the archivist before it is refused.
const DELETE_WAIT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeleteAnswer {
    Delete,
    Keep,
    TimedOut,
}

/// The one delete waiting for the archivist, and where their answer goes.
struct PendingDelete(async_channel::Sender<DeleteAnswer>);
impl Global for PendingDelete {}

/// Answer the delete waiting for the archivist, as the dialog's buttons do.
pub(crate) fn answer_pending_delete(delete: bool, cx: &mut App) {
    if let Some(pending) = cx.try_global::<PendingDelete>() {
        let answer = match delete {
            true => DeleteAnswer::Delete,
            false => DeleteAnswer::Keep,
        };
        let _ = pending.0.try_send(answer);
    }
}

enum DeleteTarget {
    Rows(Vec<settings::project::RowId>),
    Column(SharedString),
}

/// Ask the archivist, in the app, whether to delete what the agent named, showing its reason; the
/// answer comes back once they choose, or after [`DELETE_WAIT`] as a refusal.
fn ask_to_delete(
    agent: &str,
    revision: TableRevision,
    rows: Vec<usize>,
    column: Option<String>,
    reason: String,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    let prepared = (|| {
        let state = cx
            .try_global::<TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .ok_or(RequestError::TableUnavailable)?;
        let delegate = state.read(cx).delegate();
        if revision != TableRevision(delegate.values_generation()) {
            return Err(RequestError::StaleRevision);
        }
        if cx.has_global::<PendingDelete>() {
            return Err(RequestError::DeletePending);
        }
        if cx.active_window().is_none() {
            return Err(RequestError::NoWindow);
        }
        Ok(match column {
            Some(name) => {
                let col = delegate
                    .data_col(&name)
                    .ok_or(RequestError::UnknownColumn)?;
                let filled = (0..delegate.row_count())
                    .filter(|row| {
                        delegate
                            .cell(*row, col)
                            .is_some_and(|text| !text.is_empty())
                    })
                    .count();
                (
                    format!("{agent} wants to delete the column “{name}”"),
                    vec![SharedString::from(format!(
                        "It has {filled} filled cell(s) out of {}.",
                        delegate.row_count()
                    ))],
                    DeleteTarget::Column(name.into()),
                )
            }
            None => {
                let mut rows = rows;
                rows.sort_unstable();
                rows.dedup();
                if rows.iter().any(|row| *row >= delegate.row_count()) {
                    return Err(RequestError::RowOutOfRange);
                }
                let lines = rows
                    .iter()
                    .take(10)
                    .map(|row| {
                        let first = delegate
                            .row_fields(*row)
                            .into_iter()
                            .map(|(_, text)| text)
                            .find(|text| !text.trim().is_empty())
                            .unwrap_or_default();
                        SharedString::from(format!("Row {}: {first}", row + 1))
                    })
                    .chain(
                        (rows.len() > 10).then(|| format!("… and {} more", rows.len() - 10).into()),
                    )
                    .collect();
                let ids = rows
                    .iter()
                    .filter_map(|row| delegate.row_id(*row))
                    .collect();
                let title = match rows.len() {
                    1 => format!("{agent} wants to delete row {}", rows[0] + 1),
                    n => format!("{agent} wants to delete {n} rows"),
                };
                (title, lines, DeleteTarget::Rows(ids))
            }
        })
    })();
    let (title, lines, target) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return Task::ready(Err(error)),
    };

    let (answer, answers) = async_channel::bounded(3);
    cx.set_global(PendingDelete(answer.clone()));
    let timer = cx.background_executor().timer(DELETE_WAIT);
    cx.background_executor()
        .spawn(async move {
            timer.await;
            let _ = answer.try_send(DeleteAnswer::TimedOut);
        })
        .detach();
    let (title, reason) = (SharedString::from(title), SharedString::from(reason));
    let agent_label = SharedString::from(format!("Reason given by {agent}:"));
    in_active_window(cx, move |window, cx| {
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let muted = cx.theme().muted_foreground;
            alert
                .title(title.clone())
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(agent_label.clone()))
                        .child(div().child(reason.clone()))
                        .children(
                            lines
                                .iter()
                                .map(|line| div().text_sm().text_color(muted).child(line.clone())),
                        ),
                )
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Delete")
                        .ok_variant(ButtonVariant::Danger)
                        .show_cancel(true)
                        .cancel_text("Keep"),
                )
                .on_ok(|_, _, cx| {
                    answer_pending_delete(true, cx);
                    true
                })
                .on_cancel(|_, _, cx| {
                    answer_pending_delete(false, cx);
                    true
                })
                // Also runs after either button; the first answer is the one that counts.
                .on_close(|_, _, cx| answer_pending_delete(false, cx))
        })
    });

    let agent = agent.to_owned();
    cx.spawn(async move |cx| {
        let answer = answers.recv().await.unwrap_or(DeleteAnswer::Keep);
        cx.update(|cx| {
            cx.remove_global::<PendingDelete>();
            match answer {
                DeleteAnswer::Keep => return Err(RequestError::DeleteDeclined),
                DeleteAnswer::TimedOut => {
                    in_active_window(cx, |window, cx| window.close_all_dialogs(cx));
                    return Err(RequestError::DeleteTimedOut);
                }
                DeleteAnswer::Delete => {}
            }
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .ok_or(RequestError::TableUnavailable)?;
            let (count, detail) = match target {
                DeleteTarget::Column(name) => {
                    let col = state
                        .read(cx)
                        .delegate()
                        .data_col(&name)
                        .ok_or(RequestError::UnknownColumn)?;
                    let detail = format!("Delete column {name}");
                    let origin = Origin::Agent {
                        agent: agent.clone(),
                        fix: detail.clone(),
                    };
                    crate::structural(crate::Structural::DeleteColumn { col }, origin, cx);
                    (1, detail)
                }
                DeleteTarget::Rows(ids) => {
                    let rows: Vec<usize> = ids
                        .iter()
                        .filter_map(|id| state.read(cx).delegate().row_of(*id))
                        .collect();
                    let count = rows.len();
                    let detail = match count {
                        1 => "Delete a row".to_owned(),
                        n => format!("Delete {n} rows"),
                    };
                    let origin = Origin::Agent {
                        agent: agent.clone(),
                        fix: detail.clone(),
                    };
                    crate::structural(crate::Structural::DeleteRows(rows), origin, cx);
                    (count, detail)
                }
            };
            log::info!("{agent}: {detail}, confirmed by the archivist");
            Ok(Response {
                revision: TableRevision(state.read(cx).delegate().values_generation()),
                result: ResultSet::Deleted { count },
            })
        })
    })
}

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
        Finding, Overview, ProjectSummary, Query, QuerySource, Request, RequestError, ResultSet,
        TableRevision,
    };
    use diagnostics::{DATASET_MAIN, Location};
    use gpui::{AppContext as _, TestAppContext};

    use gpui::SharedString;

    use crate::agent::{agent_source, apply_renames, apply_rewrite, apply_staged, staged_actions};
    use crate::{TablePanel, TableStateHandle};

    fn respond_to_agent(
        request: Request,
        cx: &mut gpui::App,
    ) -> Result<ai::agent::Response, RequestError> {
        crate::respond_to_agent(request, "pi", cx)
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
        crate::respond_to_agent(
            Request::StageFindings {
                revision: TableRevision(0),
                findings,
            },
            agent,
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
    /// renames, staged together, applied one after the other, and put back by two undos.
    #[gpui::test]
    fn a_rewrite_and_its_renames_apply_and_two_undos_restore_everything(cx: &mut TestAppContext) {
        let dir = open_files("walkthrough", cx);
        let ids = ["AD-001", "AD-002", "AD-003"];
        cx.update(|cx| {
            let values: Vec<(usize, &str)> = ids.iter().copied().enumerate().collect();
            let request = rewrite("File", &values, cx);
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
                apply_rewrite(&agent_source("pi"), &"File".into(), cx),
                (3, 0)
            );
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(linked(0, cx), ("AD-001".into(), None), "not renamed yet");
            assert_eq!(apply_renames(&agent_source("pi"), cx), Ok((3, 0)));
        });
        cx.run_until_parked();
        cx.update(|cx| {
            for (row, id) in ids.iter().enumerate() {
                assert_eq!(linked(row, cx), ((*id).into(), Some(format!("{id}.jpg"))));
                assert_eq!(on_disk(&dir, &format!("{id}.jpg")), (row + 1).to_string());
            }
            crate::history_step(false, cx);
            crate::history_step(false, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            for row in 0..3 {
                let name = (row + 1).to_string();
                assert_eq!(linked(row, cx), (name.clone(), Some(format!("{name}.jpg"))));
                assert_eq!(on_disk(&dir, &format!("{name}.jpg")), name);
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
        let request: Request =
            serde_json::from_value(serde_json::json!({"method": "edit", "params": params}))
                .unwrap();
        respond_to_agent(request, cx).map(|response| response.result)
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
        }
        assert!(
            saves.get() >= 8,
            "History hears when pending entries reach disk"
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
                        edit: ai::agent::TableEdit::IndentRow { row: 1 },
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
            crate::respond_to_agent_async(request, "pi", cx)
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert!(window.has_active_dialog(cx), "the archivist is asked");
        })
        .unwrap();
        cx.update(|cx| {
            let second = delete(vec![0], None, cx);
            assert!(matches!(
                crate::respond_to_agent_async(second, "pi", cx),
                task if task.is_ready()
            ));
            assert_eq!(cell(1, cx), "Wharf", "nothing is deleted before the answer");
            crate::agent::answer_pending_delete(true, cx);
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
            crate::respond_to_agent_async(request, "pi", cx)
        });
        cx.run_until_parked();
        cx.update(|cx| crate::agent::answer_pending_delete(false, cx));
        assert_eq!(kept.await, Err(RequestError::DeleteDeclined));
        cx.update(|cx| assert_eq!(headers(cx), ["Title", "Medium"]));
    }

    /// A second delete while one is waiting is refused at once rather than queued behind it.
    #[gpui::test]
    async fn only_one_delete_waits_at_a_time(cx: &mut TestAppContext) {
        open_in_window(cx);
        let first = cx.update(|cx| {
            let request = delete(vec![0], None, cx);
            crate::respond_to_agent_async(request, "pi", cx)
        });
        cx.run_until_parked();
        let second = cx.update(|cx| {
            let request = delete(vec![1], None, cx);
            crate::respond_to_agent_async(request, "pi", cx)
        });
        assert_eq!(second.await, Err(RequestError::DeletePending));
        cx.update(|cx| crate::agent::answer_pending_delete(false, cx));
        assert_eq!(first.await, Err(RequestError::DeleteDeclined));
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
}
