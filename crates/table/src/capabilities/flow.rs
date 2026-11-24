//! Capabilities about the flow of a review: staging several kinds of change as one change set, and
//! following what the archivist does with the table and with your drafts.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use ai::agent::{PatchRow, Rename, RequestError, TableRevision};
use capability::{Capability, Effect, Exposure, GrantedPath, Invocation};
use gpui::{App, Global, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::review::Relink;
use super::table;

/// Rows to delete inside a change set, and why.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ChangesetDelete {
    /// Source rows, at most 1,000.
    pub rows: Vec<usize>,
    /// Shown to the archivist when they apply the change set. At most 512 bytes.
    pub reason: String,
}

/// Cell values, file renames, relinks, new rows and deletions, staged together as one draft the
/// archivist applies in one step and undoes in one step.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct StageChangeset {
    pub revision: TableRevision,
    #[serde(default)]
    pub patch: Vec<PatchRow>,
    #[serde(default)]
    pub renames: Vec<Rename>,
    #[serde(default)]
    pub relinks: Vec<Relink>,
    /// New rows, each as column name → value. At most 1,000.
    #[serde(default)]
    pub add_rows: Vec<BTreeMap<String, String>>,
    pub delete: Option<ChangesetDelete>,
    /// Shown beside the draft.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct StagedChangeset {
    pub cells: usize,
    pub renames: usize,
    pub relinks: usize,
    pub adds: usize,
    pub deletes: usize,
    /// Patch rows skipped because their `expect` no longer matches.
    pub stale: Vec<usize>,
}

pub struct StageChangesetCap;

impl Capability for StageChangesetCap {
    const NAME: &'static str = "stage_changeset";
    const SUMMARY: &'static str = "Stage a whole migration as one draft: cell values (patch), file renames, relinks, new rows and deletions. The archivist reviews it in the table and applies it in one step, confirming any deletion with your reason; one undo takes it all back. A refused change set discards your staged drafts.";
    const GROUP: &'static str = "stage";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    type Input = StageChangeset;
    type Output = StagedChangeset;

    fn revision(input: &StageChangeset) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StageChangeset) -> Result<(), RequestError> {
        ai::agent::StagePatch {
            revision: input.revision,
            rows: input.patch.clone(),
            note: input.note.clone(),
        }
        .validate()?;
        if input.add_rows.len() > ai::agent::MAX_NEW_ROWS {
            return Err(RequestError::TooManyNewRows);
        }
        if let Some(delete) = &input.delete {
            if delete.rows.is_empty() || delete.rows.len() > ai::agent::MAX_NEW_ROWS {
                return Err(RequestError::DeleteTargetUnclear);
            }
            if delete.reason.trim().is_empty() {
                return Err(RequestError::ReasonMissing);
            }
            if delete.reason.len() > ai::agent::MAX_MESSAGE_LEN {
                return Err(RequestError::ReasonTooLong);
            }
        }
        Ok(())
    }

    fn paths(input: &mut StageChangeset) -> Vec<&mut GrantedPath> {
        input
            .relinks
            .iter_mut()
            .map(|link| &mut link.path)
            .collect()
    }

    fn label(input: &StageChangeset, _: &App) -> String {
        format!(
            "change set: {} patch row(s), {} rename(s), {} relink(s), {} new row(s), {} deletion(s)",
            input.patch.len(),
            input.renames.len(),
            input.relinks.len(),
            input.add_rows.len(),
            input.delete.as_ref().map_or(0, |delete| delete.rows.len())
        )
    }

    fn outcome(output: &StagedChangeset) -> String {
        format!(
            "{} cells, {} renames, {} relinks, {} adds, {} deletes staged",
            output.cells, output.renames, output.relinks, output.adds, output.deletes
        )
    }

    fn run(
        input: StageChangeset,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<StagedChangeset, RequestError>> {
        let agent = call.agent().unwrap_or("archivist").to_owned();
        let staged = (|| {
            let relinks = input
                .relinks
                .iter()
                .map(|link| {
                    Ok((
                        link.row,
                        link.column.clone(),
                        link.path.path()?.to_path_buf(),
                    ))
                })
                .collect::<Result<Vec<_>, RequestError>>()?;
            let mut output = StagedChangeset {
                cells: 0,
                renames: 0,
                relinks: 0,
                adds: input.add_rows.len(),
                deletes: input.delete.as_ref().map_or(0, |delete| delete.rows.len()),
                stale: Vec::new(),
            };
            if !input.patch.is_empty() {
                let patch = crate::agent::stage_patch(&agent, input.patch, input.note.clone(), cx)?;
                output.cells = patch.accepted;
                output.stale = patch.stale;
            }
            if !input.renames.is_empty() {
                output.renames = crate::agent::stage_renames(&agent, input.renames, cx)?.accepted;
            }
            if !relinks.is_empty() {
                output.relinks =
                    crate::agent::stage_relinks(&agent, relinks, input.note, cx)?.accepted;
            }
            crate::agent::stage_rows(
                &agent,
                input.add_rows,
                input.delete.map(|delete| (delete.rows, delete.reason)),
                cx,
            )?;
            Ok(output)
        })();
        if staged.is_err() {
            crate::agent::discard_all(&crate::agent_source(&agent), cx);
        }
        Task::ready(staged)
    }
}

/// Something the archivist did that an agent following along may care about.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Event {
    /// Increases by one per event; pass the last you saw as `since`.
    pub seq: u64,
    /// `draft_applied`, `draft_discarded`, `change_applied` or `change_rejected`.
    pub kind: String,
    /// Whose draft it was.
    pub agent: String,
    pub detail: String,
}

#[derive(Default)]
struct Events {
    next: u64,
    log: VecDeque<Event>,
}
impl Global for Events {}

/// Record what the archivist did with an agent's draft, for agents following along.
pub(crate) fn record(kind: &str, agent: &str, detail: String, cx: &mut App) {
    let events = cx.default_global::<Events>();
    events.next += 1;
    events.log.push_back(Event {
        seq: events.next,
        kind: kind.into(),
        agent: agent.into(),
        detail,
    });
    while events.log.len() > 200 {
        events.log.pop_front();
    }
}

/// Wait until something changes.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct WaitForChange {
    /// The `revision` you last saw; a different one answers at once.
    pub revision: Option<TableRevision>,
    /// The `selection` you last saw; a different one answers at once.
    #[serde(default)]
    pub selection: Option<Vec<usize>>,
    /// The `seq` of the last event you saw.
    #[serde(default)]
    pub since: u64,
    /// How long to wait, 1 to 25 seconds. Defaults to 20.
    #[serde(default = "default_wait")]
    pub timeout_s: u64,
}

fn default_wait() -> u64 {
    20
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Changed {
    /// Whether anything changed before the wait ran out.
    pub changed: bool,
    /// Selected source rows, at most 100.
    pub selection: Vec<usize>,
    pub events: Vec<Event>,
}

pub struct WaitForChangeCap;

fn snapshot(since: u64, cx: &App) -> Option<(TableRevision, Vec<usize>, Vec<Event>)> {
    let state = table(cx).ok()?;
    let delegate = state.read(cx).delegate();
    let mut selection = delegate.selected_source_rows();
    selection.truncate(100);
    let events = cx
        .try_global::<Events>()
        .map(|events| {
            events
                .log
                .iter()
                .filter(|event| event.seq > since)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Some((
        TableRevision(delegate.values_generation()),
        selection,
        events,
    ))
}

impl Capability for WaitForChangeCap {
    const NAME: &'static str = "wait_for_change";
    const SUMMARY: &'static str = "Wait up to 25 seconds for the archivist to edit the table, change the selection, or apply or discard your drafts, then say what changed. Call it in a loop to follow along; `qrate agent watch` does that for you.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = WaitForChange;
    type Output = Changed;

    fn validate(input: &WaitForChange) -> Result<(), RequestError> {
        match (1..=25).contains(&input.timeout_s) {
            true => Ok(()),
            false => Err(RequestError::MalformedParams(
                "timeout_s must be 1 to 25".into(),
            )),
        }
    }

    fn label(input: &WaitForChange, _: &App) -> String {
        format!("wait up to {} s", input.timeout_s)
    }

    fn outcome(output: &Changed) -> String {
        match output.changed {
            true => format!("changed, {} event(s)", output.events.len()),
            false => "nothing changed".into(),
        }
    }

    fn run(
        input: WaitForChange,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<Changed, RequestError>> {
        let deadline = Instant::now() + Duration::from_secs(input.timeout_s);
        let differs =
            move |(revision, selection, events): &(TableRevision, Vec<usize>, Vec<Event>)| {
                !events.is_empty()
                    || input.revision.is_some_and(|seen| seen != *revision)
                    || input
                        .selection
                        .as_ref()
                        .is_some_and(|seen| seen != selection)
            };
        let answer =
            |(_, selection, events): (TableRevision, Vec<usize>, Vec<Event>), changed| Changed {
                changed,
                selection,
                events,
            };
        match snapshot(input.since, cx) {
            None => return Task::ready(Err(RequestError::TableUnavailable)),
            Some(now) if differs(&now) => return Task::ready(Ok(answer(now, true))),
            Some(_) => {}
        }
        let since = input.since;
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let now = cx
                    .update(|cx| snapshot(since, cx))
                    .ok_or(RequestError::TableUnavailable)?;
                let changed = differs(&now);
                if changed || Instant::now() >= deadline {
                    return Ok(answer(now, changed));
                }
            }
        })
    }
}

pub(super) fn register(cx: &mut App) {
    capability::register::<StageChangesetCap>(cx);
    capability::register::<WaitForChangeCap>(cx);
}
