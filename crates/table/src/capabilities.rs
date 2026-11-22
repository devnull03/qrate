//! The table's features as [`capability::Capability`]s: what agents, the CLI and the generated docs
//! can use, each with its effect declared once. [`init`] registers them and installs the table as
//! the dispatcher's [`capability::Host`].

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ai::agent::{
    AddColumn, AddRows, CheckAccess, Delete, Deleted, DescribeColumn, Edited, FileAccess,
    FileAccessRevoked, Filter, ImportFiles, ImportedFiles, MoveColumn, MoveRow, NoInput, Overview,
    ProgramAudit, ProgramOutput, ProgramRun, ProgramSave, ProgramSaved, Query, QueryResult,
    QuerySource, RenameColumn, RequestAccess, RequestError, RowEdit, RowPlacement,
    StageColumnRewrite, StageFindings, StageRenames, Staged, StagedRenames, StagedRewrite,
    TableRevision, ThumbnailSet, Thumbnails,
};
use base64::Engine as _;
use capability::{Capability, Confirmation, Effect, Exposure, Invocation, Reach};
use gpui::{App, Entity, Global, ParentElement as _, SharedString, Styled as _, Task, div};
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::table::TableState;
use gpui_component::{ActiveTheme as _, WindowExt as _, v_flex};
use sha2::{Digest as _, Sha256};

use crate::agent::{AgentProgram, PROGRAM_RUNS, in_active_window};
use crate::{QrateTableDelegate, TableStateHandle};

pub(crate) fn table(cx: &App) -> Result<Entity<TableState<QrateTableDelegate>>, RequestError> {
    cx.try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)
}

fn current_revision(cx: &App) -> Result<TableRevision, RequestError> {
    Ok(TableRevision(
        table(cx)?.read(cx).delegate().values_generation(),
    ))
}

fn column(name: &str, cx: &App) -> Result<usize, RequestError> {
    table(cx)?
        .read(cx)
        .delegate()
        .data_col(name)
        .ok_or(RequestError::UnknownColumn)
}

fn row(row: usize, cx: &App) -> Result<usize, RequestError> {
    match row < table(cx)?.read(cx).delegate().row_count() {
        true => Ok(row),
        false => Err(RequestError::RowOutOfRange),
    }
}

fn new_column_name(name: &str, cx: &App) -> Result<SharedString, RequestError> {
    let name = name.trim();
    if name.is_empty() || table(cx)?.read(cx).delegate().data_col(name).is_some() {
        return Err(RequestError::ColumnNameTaken(name.to_owned()));
    }
    Ok(SharedString::from(name.to_owned()))
}

fn edited(call: &Invocation, rows: Vec<usize>) -> Task<Result<Edited, RequestError>> {
    log::info!("{}: {}", call.agent().unwrap_or("archivist"), call.label);
    Task::ready(Ok(Edited {
        detail: call.label.clone(),
        rows,
    }))
}

fn plural(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => one.to_owned(),
        n => format!("{n} {many}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Host

struct TableHost;

/// The one confirmation waiting for the archivist, and where their answer goes.
struct PendingConfirm(async_channel::Sender<Answer>);
impl Global for PendingConfirm {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answer {
    Allow,
    Keep,
    TimedOut,
}

/// How long a confirmation waits for the archivist before it is refused.
const CONFIRM_WAIT: Duration = Duration::from_secs(10 * 60);

/// Answer the confirmation waiting for the archivist, as the dialog's buttons do.
pub(crate) fn answer_pending_confirm(allow: bool, cx: &mut App) {
    if let Some(pending) = cx.try_global::<PendingConfirm>() {
        let _ = pending.0.try_send(match allow {
            true => Answer::Allow,
            false => Answer::Keep,
        });
    }
}

impl capability::Host for TableHost {
    fn revision(&self, cx: &App) -> Option<TableRevision> {
        current_revision(cx).ok()
    }

    fn history_mark(&self, cx: &App) -> usize {
        table(cx).map_or(0, |state| state.read(cx).delegate().undo_depth())
    }

    fn settle_history(&self, mark: usize, cx: &mut App) {
        if let Ok(state) = table(cx) {
            state.update(cx, |state, _| state.delegate_mut().squash_undo_since(mark));
        }
    }

    fn confirm(&self, ask: Confirmation, cx: &mut App) -> Task<Result<(), RequestError>> {
        if cx.has_global::<PendingConfirm>() {
            return Task::ready(Err(RequestError::ConfirmPending));
        }
        if cx.active_window().is_none() {
            return Task::ready(Err(RequestError::NoWindow));
        }
        let (answer, answers) = async_channel::bounded(3);
        cx.set_global(PendingConfirm(answer.clone()));
        let timer = cx.background_executor().timer(CONFIRM_WAIT);
        cx.background_executor()
            .spawn(async move {
                timer.await;
                let _ = answer.try_send(Answer::TimedOut);
            })
            .detach();
        let mut label = ask.title.chars();
        let lowered: String = label
            .next()
            .map(|first| first.to_lowercase().chain(label).collect())
            .unwrap_or_default();
        let title = SharedString::from(format!("{} wants to {lowered}", ask.agent));
        let destructive = ask.destructive;
        let reason = SharedString::from(ask.reason);
        let by = SharedString::from(format!("Reason given by {}:", ask.agent));
        let lines: Vec<SharedString> = ask.details.into_iter().map(Into::into).collect();
        in_active_window(cx, move |window, cx| {
            window.open_alert_dialog(cx, move |alert, _, cx| {
                let muted = cx.theme().muted_foreground;
                alert
                    .title(title.clone())
                    .description(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().text_color(muted).child(by.clone()))
                            .child(div().child(reason.clone()))
                            .children(
                                lines.iter().map(|line| {
                                    div().text_sm().text_color(muted).child(line.clone())
                                }),
                            ),
                    )
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text(match destructive {
                                true => "Delete",
                                false => "Allow",
                            })
                            .ok_variant(match destructive {
                                true => ButtonVariant::Danger,
                                false => ButtonVariant::Primary,
                            })
                            .show_cancel(true)
                            .cancel_text("Keep"),
                    )
                    .on_ok(|_, _, cx| {
                        answer_pending_confirm(true, cx);
                        true
                    })
                    .on_cancel(|_, _, cx| {
                        answer_pending_confirm(false, cx);
                        true
                    })
                    // Also runs after either button; the first answer is the one that counts.
                    .on_close(|_, _, cx| answer_pending_confirm(false, cx))
            })
        });
        cx.spawn(async move |cx| {
            let answer = answers.recv().await.unwrap_or(Answer::Keep);
            cx.update(|cx| {
                cx.remove_global::<PendingConfirm>();
                match answer {
                    Answer::Allow => Ok(()),
                    Answer::Keep => Err(RequestError::Declined),
                    Answer::TimedOut => {
                        in_active_window(cx, |window, cx| window.close_all_dialogs(cx));
                        Err(RequestError::ConfirmTimedOut)
                    }
                }
            })
        })
    }

    fn resolve_path(
        &self,
        agent: Option<&str>,
        raw: &str,
        cx: &App,
    ) -> Result<PathBuf, RequestError> {
        crate::agent_files::resolve_granted(agent, raw, cx)
    }
}

// ---------------------------------------------------------------------------------------------
// Reads

pub struct OverviewCap;

impl Capability for OverviewCap {
    const NAME: &'static str = "overview";
    const SUMMARY: &'static str = "Read the open project's summary, columns with their types and descriptions, selection count, diagnostic counts and the exact revision. Start here.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = NoInput;
    type Output = Overview;

    fn label(_: &NoInput, _: &App) -> String {
        String::new()
    }

    fn outcome(output: &Overview) -> String {
        format!(
            "{} rows × {} columns",
            output.project.row_count, output.project.column_count
        )
    }

    fn run(_: NoInput, _: &Invocation, cx: &mut App) -> Task<Result<Overview, RequestError>> {
        Task::ready((|| {
            let project = cx
                .try_global::<settings::project::CurrentProject>()
                .ok_or(RequestError::ProjectUnavailable)?;
            let state = table(cx)?;
            Ok(crate::agent::overview(
                project,
                state.read(cx).delegate(),
                cx,
            ))
        })())
    }
}

pub struct QueryCap;

impl Capability for QueryCap {
    const NAME: &'static str = "query";
    const SUMMARY: &'static str = "Read a bounded page of rows or diagnostics, optionally filtered, grouped, counted or distinct. Follow next_cursor only when more evidence matters.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = Query;
    type Output = QueryResult;

    fn validate(input: &Query) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &Query, _: &App) -> String {
        format!("{:?}, max {}", input.source, input.limit)
    }

    fn outcome(output: &QueryResult) -> String {
        format!(
            "{} returned, {} remaining",
            output.returned, output.remaining
        )
    }

    fn run(query: Query, _: &Invocation, cx: &mut App) -> Task<Result<QueryResult, RequestError>> {
        let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
            return Task::ready(Err(RequestError::ProjectUnavailable));
        };
        let state = match table(cx) {
            Ok(state) => state,
            Err(error) => return Task::ready(Err(error)),
        };
        let state = state.read(cx);
        let delegate = state.delegate();
        let revision = TableRevision(delegate.values_generation());
        let diagnostic_source = matches!(query.source, QuerySource::Diagnostics { .. });
        let schema = match diagnostic_source {
            true => ["row", "column", "severity", "source", "message"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            false => std::iter::once("row".into())
                .chain(project.data.headers.iter().cloned())
                .collect(),
        };
        let records = match &query.source {
            QuerySource::Diagnostics {
                severities,
                sources,
            } => crate::agent::diagnostic_records(cx, severities, sources),
            source => crate::agent::row_records(delegate, source),
        };
        // Small tables answer at once; only a large one is worth a trip to a background thread.
        if records.len() <= 5_000 {
            return Task::ready(crate::agent::run_query_records(
                query,
                revision,
                records,
                schema,
                diagnostic_source,
            ));
        }
        cx.background_executor().spawn(async move {
            crate::agent::run_query_records(query, revision, records, schema, diagnostic_source)
        })
    }
}

pub struct ProgramSaveCap;

impl Capability for ProgramSaveCap {
    const NAME: &'static str = "program_save";
    const SUMMARY: &'static str = "Compile and keep one read-only Luau scratch program, for analysis bounded queries cannot do efficiently.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    const RESULT: &'static str = "program_saved";
    type Input = ProgramSave;
    type Output = ProgramSaved;

    fn validate(input: &ProgramSave) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &ProgramSave, _: &App) -> String {
        format!("{} source byte(s)", input.source.len())
    }

    fn outcome(output: &ProgramSaved) -> String {
        format!("program v{} {}", output.version, output.hash)
    }

    fn run(
        input: ProgramSave,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<ProgramSaved, RequestError>> {
        let version = cx
            .try_global::<AgentProgram>()
            .map_or(1, |program| program.version + 1);
        let source = input.source;
        // Compiling is checked, never run, and the source is capped at 64 KB: quick enough inline.
        Task::ready((|| {
            plugin_host::validate_agent_program(&source).map_err(|error| {
                log::warn!("agent scratch program was not activated: {error}");
                RequestError::ProgramCompileFailed(error.detail)
            })?;
            let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
            cx.set_global(AgentProgram {
                source,
                version,
                hash: hash.clone(),
            });
            Ok(ProgramSaved { version, hash })
        })())
    }
}

pub struct ProgramRunCap;

impl Capability for ProgramRunCap {
    const NAME: &'static str = "program_run";
    const SUMMARY: &'static str =
        "Run the saved scratch program against the live table with JSON args.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = ProgramRun;
    type Output = ProgramOutput;

    fn revision(input: &ProgramRun) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn label(input: &ProgramRun, _: &App) -> String {
        format!("at revision {}", input.revision.0)
    }

    fn outcome(output: &ProgramOutput) -> String {
        format!("program v{} in {} ms", output.version, output.elapsed_ms)
    }

    fn run(
        input: ProgramRun,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<ProgramOutput, RequestError>> {
        let state = match table(cx) {
            Ok(state) => state,
            Err(error) => return Task::ready(Err(error)),
        };
        let delegate = state.read(cx).delegate();
        let revision = TableRevision(delegate.values_generation());
        let Some(program) = cx.try_global::<AgentProgram>() else {
            return Task::ready(Err(RequestError::ProgramUnavailable));
        };
        let (version, hash, source) = (
            program.version,
            program.hash.clone(),
            program.source.clone(),
        );
        let snapshot = crate::agent::program_snapshot(revision, delegate, cx);
        let args = input.args;
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
                        "agent program run {run_id} revision {} version {version} sha256 {hash} status {} elapsed_ms {}",
                        revision.0, error.kind, error.elapsed_ms,
                    );
                    return Err(match error.kind == "output_too_large" {
                        true => RequestError::ProgramOutputTooLarge(error.detail),
                        false => RequestError::ProgramFailed(error.detail),
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
                "agent program run {} revision {} version {} sha256 {} status success elapsed_ms {} output_bytes {}",
                audit.run_id, revision.0, version, hash, audit.elapsed_ms, audit.output_bytes,
            );
            Ok(ProgramOutput {
                version,
                hash,
                value: output.value,
                logs: output.logs,
                elapsed_ms: output.elapsed_ms,
                audit,
            })
        })
    }
}

pub struct ThumbnailsCap;

impl Capability for ThumbnailsCap {
    const NAME: &'static str = "thumbnails";
    const SUMMARY: &'static str = "Read small PNG previews of up to four rows' linked files, when the image itself is the evidence.";
    const GROUP: &'static str = "media";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = Thumbnails;
    type Output = ThumbnailSet;

    fn validate(input: &Thumbnails) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &Thumbnails, _: &App) -> String {
        format!("{} thumbnail(s)", input.items.len())
    }

    fn outcome(output: &ThumbnailSet) -> String {
        format!("{} thumbnail(s)", output.items.len())
    }

    fn run(
        input: Thumbnails,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<ThumbnailSet, RequestError>> {
        let state = match table(cx) {
            Ok(state) => state,
            Err(error) => return Task::ready(Err(error)),
        };
        let delegate = state.read(cx).delegate();
        let paths: Result<Vec<_>, _> = input
            .items
            .into_iter()
            .map(|item| {
                delegate
                    .row_image(item.row)
                    .map(|path| (item, path.to_path_buf()))
                    .ok_or(RequestError::ThumbnailUnavailable)
            })
            .collect();
        let paths = match paths {
            Ok(paths) => paths,
            Err(error) => return Task::ready(Err(error)),
        };
        cx.background_executor().spawn(async move {
            let mut items = Vec::with_capacity(paths.len());
            for (item, path) in paths {
                let bytes = preview::thumbnail_png(&path, item.page)
                    .ok_or(RequestError::ThumbnailUnavailable)?;
                items.push(ai::agent::Thumbnail {
                    row: item.row,
                    page: item.page,
                    media_type: "image/png".into(),
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
            Ok(ThumbnailSet { items })
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Drafts

pub struct StageFindingsCap;

impl Capability for StageFindingsCap {
    const NAME: &'static str = "stage_findings";
    const SUMMARY: &'static str = "Publish one complete batch of advisory findings to the Problems panel, each with an optional whole-cell replacement the archivist can apply.";
    const GROUP: &'static str = "stage";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    const CHECKS_OWN_CELLS: bool = true;
    const RESULT: &'static str = "staged";
    type Input = StageFindings;
    type Output = Staged;

    fn revision(input: &StageFindings) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StageFindings) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &StageFindings, _: &App) -> String {
        format!(
            "{} finding(s) at revision {}",
            input.findings.len(),
            input.revision.0
        )
    }

    fn outcome(output: &Staged) -> String {
        format!("{} staged, {} stale", output.accepted, output.stale.len())
    }

    fn run(
        input: StageFindings,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<Staged, RequestError>> {
        Task::ready(crate::agent::stage_findings(
            call.agent().unwrap_or("archivist"),
            input.revision,
            input.findings,
            cx,
        ))
    }
}

pub struct StageColumnRewriteCap;

impl Capability for StageColumnRewriteCap {
    const NAME: &'static str = "stage_column_rewrite";
    const SUMMARY: &'static str = "Stage new values for one whole column as a single draft the archivist previews and applies. Never edit a column cell by cell.";
    const GROUP: &'static str = "stage";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    const RESULT: &'static str = "staged_rewrite";
    type Input = StageColumnRewrite;
    type Output = StagedRewrite;

    fn revision(input: &StageColumnRewrite) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StageColumnRewrite) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &StageColumnRewrite, _: &App) -> String {
        format!("{} value(s) for {}", input.values.len(), input.column)
    }

    fn outcome(output: &StagedRewrite) -> String {
        format!(
            "{} staged, {} unchanged",
            output.accepted,
            output.unchanged.len()
        )
    }

    fn run(
        input: StageColumnRewrite,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<StagedRewrite, RequestError>> {
        Task::ready(crate::agent::stage_column_rewrite(
            call.agent().unwrap_or("archivist"),
            input.column,
            input.values,
            input.note,
            cx,
        ))
    }
}

pub struct StageRenamesCap;

impl Capability for StageRenamesCap {
    const NAME: &'static str = "stage_renames";
    const SUMMARY: &'static str = "Stage renames of files rows link to, inside the project folder, as one draft. Applying renames all or none and updates the rows; undo renames them back.";
    const GROUP: &'static str = "stage";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    const RESULT: &'static str = "staged_renames";
    type Input = StageRenames;
    type Output = StagedRenames;

    fn revision(input: &StageRenames) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StageRenames) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &StageRenames, _: &App) -> String {
        format!("{} rename(s)", input.renames.len())
    }

    fn outcome(output: &StagedRenames) -> String {
        format!("{} rename(s) staged", output.accepted)
    }

    fn run(
        input: StageRenames,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<StagedRenames, RequestError>> {
        Task::ready(crate::agent::stage_renames(
            call.agent().unwrap_or("archivist"),
            input.renames,
            cx,
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Shape edits

macro_rules! edit_capability {
    ($ty:ident, $name:literal, $summary:literal, $input:ty) => {
        impl Capability for $ty {
            const NAME: &'static str = $name;
            const SUMMARY: &'static str = $summary;
            const GROUP: &'static str = "table";
            const EXPOSURE: Exposure = Exposure::Agent;
            const EFFECT: Effect = Effect::Edit;
            const RESULT: &'static str = "edited";
            type Input = $input;
            type Output = Edited;

            fn revision(input: &$input) -> Option<TableRevision> {
                Some(input.revision)
            }

            fn validate(input: &$input) -> Result<(), RequestError> {
                $ty::check(input)
            }

            fn label(input: &$input, _: &App) -> String {
                $ty::describe(input)
            }

            fn outcome(output: &Edited) -> String {
                output.detail.clone()
            }

            fn run(
                input: $input,
                call: &Invocation,
                cx: &mut App,
            ) -> Task<Result<Edited, RequestError>> {
                match $ty::apply(input, call, cx) {
                    Ok(rows) => edited(call, rows),
                    Err(error) => Task::ready(Err(error)),
                }
            }
        }
    };
}

pub struct AddRowsCap;
edit_capability!(
    AddRowsCap,
    "add_rows",
    "Append up to 1,000 rows, each as column name → value. One undo step.",
    AddRows
);

impl AddRowsCap {
    fn check(input: &AddRows) -> Result<(), RequestError> {
        input.validate()
    }

    fn describe(input: &AddRows) -> String {
        match input.rows.len() {
            1 => "Add a row".into(),
            n => format!("Add {n} rows"),
        }
    }

    fn apply(input: AddRows, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let state = table(cx)?;
        let origin = call.origin();
        let delegate = state.read(cx).delegate();
        let width = delegate.column_count();
        let mut values = Vec::with_capacity(input.rows.len());
        for fields in &input.rows {
            let mut cells = vec![SharedString::default(); width];
            for (name, value) in fields {
                cells[column(name, cx)?] = value.clone().into();
            }
            values.push(cells);
        }
        let at = state.read(cx).delegate().row_count();
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
        diagnostics::Diagnostics::align_note_rows(diagnostics::DATASET_MAIN, &row_ids, origin, cx);
        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
        crate::revalidate_now(cx);
        crate::autosave(cx);
        Ok((at..at + added).collect())
    }
}

pub struct AddColumnCap;
edit_capability!(
    AddColumnCap,
    "add_column",
    "Add a new, empty column after `after`, or first.",
    AddColumn
);

impl AddColumnCap {
    fn check(_: &AddColumn) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &AddColumn) -> String {
        format!("Add column {}", input.name.trim())
    }

    fn apply(
        input: AddColumn,
        call: &Invocation,
        cx: &mut App,
    ) -> Result<Vec<usize>, RequestError> {
        let name = new_column_name(&input.name, cx)?;
        let at = match &input.after {
            Some(after) => column(after, cx)? + 1,
            None => 0,
        };
        crate::structural(
            crate::Structural::AddNamedColumn { at, name },
            call.origin(),
            cx,
        );
        Ok(Vec::new())
    }
}

pub struct RenameColumnCap;
edit_capability!(
    RenameColumnCap,
    "rename_column",
    "Rename a column to a name not already used.",
    RenameColumn
);

impl RenameColumnCap {
    fn check(_: &RenameColumn) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &RenameColumn) -> String {
        format!("Rename column {} to {}", input.column, input.name.trim())
    }

    fn apply(
        input: RenameColumn,
        call: &Invocation,
        cx: &mut App,
    ) -> Result<Vec<usize>, RequestError> {
        let col = column(&input.column, cx)?;
        let name = new_column_name(&input.name, cx)?;
        crate::structural(
            crate::Structural::RenameColumn { col, name },
            call.origin(),
            cx,
        );
        Ok(Vec::new())
    }
}

pub struct MoveColumnCap;
edit_capability!(
    MoveColumnCap,
    "move_column",
    "Move a column after `after`, or to the front.",
    MoveColumn
);

impl MoveColumnCap {
    fn check(_: &MoveColumn) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &MoveColumn) -> String {
        format!("Move column {}", input.column)
    }

    fn apply(
        input: MoveColumn,
        call: &Invocation,
        cx: &mut App,
    ) -> Result<Vec<usize>, RequestError> {
        let from = column(&input.column, cx)?;
        let to = match &input.after {
            Some(after) => {
                let after = column(after, cx)?;
                if after < from { after + 1 } else { after }
            }
            None => 0,
        };
        crate::structural(
            crate::Structural::MoveColumn { from, to },
            call.origin(),
            cx,
        );
        Ok(Vec::new())
    }
}

pub struct DescribeColumnCap;
edit_capability!(
    DescribeColumnCap,
    "describe_column",
    "Set a column's data type, its description, or both.",
    DescribeColumn
);

impl DescribeColumnCap {
    fn check(_: &DescribeColumn) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &DescribeColumn) -> String {
        format!("Describe column {}", input.column)
    }

    fn apply(
        input: DescribeColumn,
        call: &Invocation,
        cx: &mut App,
    ) -> Result<Vec<usize>, RequestError> {
        column(&input.column, cx)?;
        let kind = match &input.data_type {
            Some(data_type) => Some(
                settings::columns::ColumnType::ALL
                    .into_iter()
                    .find(|kind| kind.as_str().eq_ignore_ascii_case(data_type.trim()))
                    .ok_or_else(|| RequestError::UnknownDataType(data_type.clone()))?,
            ),
            None => None,
        };
        match crate::describe_column(
            &input.column,
            kind.map(|kind| kind.as_str()),
            input.description.as_deref().map(str::trim),
            call.origin(),
            cx,
        ) {
            true => Ok(Vec::new()),
            false => Err(RequestError::UnknownColumn),
        }
    }
}

pub struct MoveRowCap;
edit_capability!(
    MoveRowCap,
    "move_row",
    "Move a row before or after another row, or make it that row's child.",
    MoveRow
);

impl MoveRowCap {
    fn check(_: &MoveRow) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &MoveRow) -> String {
        format!("Move row {}", input.row + 1)
    }

    fn apply(input: MoveRow, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let (moved, target) = (row(input.row, cx)?, row(input.target, cx)?);
        let placement = match input.placement {
            RowPlacement::Before => crate::RowPlacement::Before,
            RowPlacement::After => crate::RowPlacement::After,
            RowPlacement::Child => crate::RowPlacement::Child,
        };
        let op = crate::Arrangement::Move {
            row: moved,
            target,
            placement,
        };
        match crate::arrange(op, call.origin(), cx) {
            true => Ok(Vec::new()),
            false => Err(RequestError::EditRefused("the row cannot go there".into())),
        }
    }
}

pub struct IndentRowCap;
edit_capability!(
    IndentRowCap,
    "indent_row",
    "Make a row a child of the row above it at its level.",
    RowEdit
);

impl IndentRowCap {
    fn check(_: &RowEdit) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &RowEdit) -> String {
        format!("Indent row {}", input.row + 1)
    }

    fn apply(input: RowEdit, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let indented = row(input.row, cx)?;
        match crate::arrange(crate::Arrangement::Indent(indented), call.origin(), cx) {
            true => Ok(Vec::new()),
            false => Err(RequestError::EditRefused(
                "the row has no row above it at its level".into(),
            )),
        }
    }
}

pub struct OutdentRowCap;
edit_capability!(
    OutdentRowCap,
    "outdent_row",
    "Move a row up one level of the hierarchy.",
    RowEdit
);

impl OutdentRowCap {
    fn check(_: &RowEdit) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &RowEdit) -> String {
        format!("Outdent row {}", input.row + 1)
    }

    fn apply(input: RowEdit, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let outdented = row(input.row, cx)?;
        match crate::arrange(crate::Arrangement::Outdent(outdented), call.origin(), cx) {
            true => Ok(Vec::new()),
            false => Err(RequestError::EditRefused(
                "the row is already at the top level".into(),
            )),
        }
    }
}

pub struct DuplicateRowCap;
edit_capability!(
    DuplicateRowCap,
    "duplicate_row",
    "Copy a row in below itself.",
    RowEdit
);

impl DuplicateRowCap {
    fn check(_: &RowEdit) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &RowEdit) -> String {
        format!("Duplicate row {}", input.row + 1)
    }

    fn apply(input: RowEdit, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let copied = row(input.row, cx)?;
        crate::structural(
            crate::Structural::DuplicateRow { row: copied },
            call.origin(),
            cx,
        );
        Ok(vec![copied + 1])
    }
}

pub struct FilterCap;
edit_capability!(
    FilterCap,
    "filter",
    "Show only rows whose column holds one of `keep`, or clear that column's filter.",
    Filter
);

impl FilterCap {
    fn check(_: &Filter) -> Result<(), RequestError> {
        Ok(())
    }

    fn describe(input: &Filter) -> String {
        match input.keep {
            Some(_) => format!("Filter {}", input.column),
            None => format!("Clear the filter on {}", input.column),
        }
    }

    fn apply(input: Filter, call: &Invocation, cx: &mut App) -> Result<Vec<usize>, RequestError> {
        let col = column(&input.column, cx)?;
        let origin = call.origin();
        table(cx)?.update(cx, |state, cx| {
            let keep: Option<Vec<SharedString>> = input
                .keep
                .as_ref()
                .map(|values| values.iter().cloned().map(Into::into).collect());
            state
                .delegate_mut()
                .filter_from_agent(col, keep.as_deref(), origin);
            state.refresh(cx);
            cx.emit(crate::TableChanged);
            cx.notify();
        });
        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
        crate::autosave(cx);
        Ok(Vec::new())
    }
}

pub struct DeleteCap;

impl Capability for DeleteCap {
    const NAME: &'static str = "delete";
    const SUMMARY: &'static str = "Ask the archivist to delete rows or one column, showing your reason. Waits until they answer; if they decline, do not ask again for the same thing.";
    const GROUP: &'static str = "table";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Confirm;
    const RESULT: &'static str = "deleted";
    const DESTRUCTIVE: bool = true;
    type Input = Delete;
    type Output = Deleted;

    fn revision(input: &Delete) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &Delete) -> Result<(), RequestError> {
        input.validate()
    }

    fn reason(input: &Delete) -> Option<&str> {
        Some(&input.reason)
    }

    fn label(input: &Delete, _: &App) -> String {
        match (&input.column, input.rows.as_slice()) {
            (Some(column), _) => format!("Delete the column “{column}”"),
            (None, [row]) => format!("Delete row {}", row + 1),
            (None, rows) => format!("Delete {} rows", rows.len()),
        }
    }

    fn details(input: &Delete, cx: &App) -> Vec<String> {
        let Ok(state) = table(cx) else {
            return Vec::new();
        };
        let delegate = state.read(cx).delegate();
        if let Some(name) = &input.column {
            let Some(col) = delegate.data_col(name) else {
                return Vec::new();
            };
            let filled = (0..delegate.row_count())
                .filter(|row| {
                    delegate
                        .cell(*row, col)
                        .is_some_and(|text| !text.is_empty())
                })
                .count();
            return vec![format!(
                "It has {filled} filled cell(s) out of {}.",
                delegate.row_count()
            )];
        }
        let shown = input.rows.iter().take(10).map(|row| {
            let first = delegate
                .row_fields(*row)
                .into_iter()
                .map(|(_, text)| text)
                .find(|text| !text.trim().is_empty())
                .unwrap_or_default();
            format!("Row {}: {first}", row + 1)
        });
        shown
            .chain((input.rows.len() > 10).then(|| format!("… and {} more", input.rows.len() - 10)))
            .collect()
    }

    fn outcome(output: &Deleted) -> String {
        format!("{} deleted, confirmed in the app", output.count)
    }

    fn run(input: Delete, call: &Invocation, cx: &mut App) -> Task<Result<Deleted, RequestError>> {
        Task::ready((|| {
            let origin = call.origin();
            if let Some(name) = &input.column {
                let col = column(name, cx)?;
                crate::structural(crate::Structural::DeleteColumn { col }, origin, cx);
                log::info!("{}: {}", call.agent().unwrap_or("archivist"), call.label);
                return Ok(Deleted { count: 1 });
            }
            let mut rows = input.rows;
            rows.sort_unstable();
            rows.dedup();
            for row_ix in &rows {
                row(*row_ix, cx)?;
            }
            let count = rows.len();
            crate::structural(crate::Structural::DeleteRows(rows), origin, cx);
            log::info!("{}: {}", call.agent().unwrap_or("archivist"), call.label);
            Ok(Deleted { count })
        })())
    }
}

// ---------------------------------------------------------------------------------------------
// Files

pub struct RequestAccessCap;

impl Capability for RequestAccessCap {
    const NAME: &'static str = "request_access";
    const SUMMARY: &'static str = "Ask the archivist for read-only access to one file or folder outside the project, with a reason. Lasts until qrate closes or they revoke it.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Reach(Reach::Files);
    const RESULT: &'static str = "file_access";
    type Input = RequestAccess;
    type Output = FileAccess;

    fn validate(input: &RequestAccess) -> Result<(), RequestError> {
        input.validate()?;
        match input.reason.trim().is_empty() {
            true => Err(RequestError::ReasonMissing),
            false if input.reason.len() > ai::agent::MAX_MESSAGE_LEN => {
                Err(RequestError::ReasonTooLong)
            }
            false => Ok(()),
        }
    }

    fn label(input: &RequestAccess, _: &App) -> String {
        format!("read access to {}", input.path)
    }

    fn outcome(output: &FileAccess) -> String {
        match output.directory {
            true => "folder read access".into(),
            false => "file read access".into(),
        }
    }

    fn run(
        input: RequestAccess,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<FileAccess, RequestError>> {
        crate::agent_files::request_access(
            call.agent().unwrap_or("archivist"),
            input.path,
            input.reason,
            cx,
        )
    }
}

pub struct CheckAccessCap;

impl Capability for CheckAccessCap {
    const NAME: &'static str = "check_access";
    const SUMMARY: &'static str =
        "Check whether a path is readable without asking or widening any grant.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Reach(Reach::Files);
    const RESULT: &'static str = "file_access";
    type Input = CheckAccess;
    type Output = FileAccess;

    fn validate(input: &CheckAccess) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &CheckAccess, _: &App) -> String {
        format!("check read access to {}", input.path)
    }

    fn run(
        input: CheckAccess,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<FileAccess, RequestError>> {
        crate::agent_files::check_access(call.agent().unwrap_or("archivist"), input.path, cx)
    }
}

pub struct ForgetAccessCap;

impl Capability for ForgetAccessCap {
    const NAME: &'static str = "forget_access";
    const SUMMARY: &'static str = "Give up every read grant you hold for this project.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Reach(Reach::Files);
    const RESULT: &'static str = "file_access_revoked";
    type Input = NoInput;
    type Output = FileAccessRevoked;

    fn label(_: &NoInput, _: &App) -> String {
        "revoke read access".into()
    }

    fn outcome(output: &FileAccessRevoked) -> String {
        format!("{} read grant(s) revoked", output.count)
    }

    fn run(
        _: NoInput,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<FileAccessRevoked, RequestError>> {
        Task::ready(crate::agent_files::forget_access(
            call.agent().unwrap_or("archivist"),
            cx,
        ))
    }
}

pub struct ImportFilesCap;

impl Capability for ImportFilesCap {
    const NAME: &'static str = "import_files";
    const SUMMARY: &'static str = "Attach readable files and folders as new rows, linking originals in place and skipping files already in the project. One undo step.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Edit;
    const RESULT: &'static str = "imported_files";
    type Input = ImportFiles;
    type Output = ImportedFiles;

    fn revision(input: &ImportFiles) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &ImportFiles) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &ImportFiles, _: &App) -> String {
        format!("Import {}", plural(input.paths.len(), "a path", "paths"))
    }

    fn outcome(output: &ImportedFiles) -> String {
        format!(
            "{} added, {} existing components skipped",
            output.added, output.skipped
        )
    }

    fn run(
        input: ImportFiles,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<ImportedFiles, RequestError>> {
        crate::agent_files::import_files(
            call.agent().unwrap_or("archivist"),
            input.revision,
            input.paths,
            input.recursive,
            call.origin(),
            cx,
        )
    }
}

/// Register every table capability and make the table the dispatcher's host.
pub fn init(cx: &mut App) {
    capability::set_host(TableHost, cx);
    capability::register::<OverviewCap>(cx);
    capability::register::<QueryCap>(cx);
    capability::register::<ProgramSaveCap>(cx);
    capability::register::<ProgramRunCap>(cx);
    capability::register::<ThumbnailsCap>(cx);
    capability::register::<StageFindingsCap>(cx);
    capability::register::<StageColumnRewriteCap>(cx);
    capability::register::<StageRenamesCap>(cx);
    capability::register::<AddRowsCap>(cx);
    capability::register::<AddColumnCap>(cx);
    capability::register::<RenameColumnCap>(cx);
    capability::register::<MoveColumnCap>(cx);
    capability::register::<DescribeColumnCap>(cx);
    capability::register::<MoveRowCap>(cx);
    capability::register::<IndentRowCap>(cx);
    capability::register::<OutdentRowCap>(cx);
    capability::register::<DuplicateRowCap>(cx);
    capability::register::<FilterCap>(cx);
    capability::register::<DeleteCap>(cx);
    capability::register::<RequestAccessCap>(cx);
    capability::register::<CheckAccessCap>(cx);
    capability::register::<ForgetAccessCap>(cx);
    capability::register::<ImportFilesCap>(cx);
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    /// `docs/cli/capabilities.md` is generated from the catalog; this fails when it is stale.
    /// Regenerate it with `QRATE_BLESS=1 cargo test -p table capability_reference`.
    #[gpui::test]
    fn capability_reference_matches_the_catalog(cx: &mut TestAppContext) {
        let page = cx.update(|cx| {
            capability::init(cx);
            super::init(cx);
            capability::markdown(&capability::catalog(cx))
        });
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cli/capabilities.md");
        if std::env::var_os("QRATE_BLESS").is_some() {
            std::fs::write(&path, &page).unwrap();
        }
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            written == page,
            "docs/cli/capabilities.md is stale: run QRATE_BLESS=1 cargo test -p table capability_reference"
        );
    }

    /// Every capability declares what an agent needs to call it well.
    #[gpui::test]
    fn every_capability_is_documented_and_answers_an_object(cx: &mut TestAppContext) {
        let catalog = cx.update(|cx| {
            capability::init(cx);
            super::init(cx);
            capability::catalog(cx)
        });
        for descriptor in catalog {
            assert!(
                !descriptor.summary.is_empty(),
                "{} has no summary",
                descriptor.name
            );
            assert_eq!(
                descriptor.input["type"], "object",
                "{} takes an object",
                descriptor.name
            );
            assert_eq!(
                descriptor.output["type"], "object",
                "{} answers an object",
                descriptor.name
            );
            let needs_revision = matches!(descriptor.effect.as_str(), "stage" | "edit" | "confirm");
            assert!(
                !needs_revision || descriptor.input["properties"]["revision"].is_object(),
                "{} changes the project, so it must take the revision it read",
                descriptor.name
            );
        }
    }
}
