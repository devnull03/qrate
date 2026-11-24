//! Capabilities for reviewing a collection: multi-column drafts, variant spellings, the History,
//! notes, and the files rows link to.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ai::agent::{
    AddNote, Candidate, CandidateSet, Candidates, Count, Edited, HistoryItem, HistoryPage,
    HistoryQuery, NoteItem, NotesPage, NotesQuery, RequestError, StagePatch, StagedPatch,
    StagedRelinks, TableRevision, ValueVariants, VariantGroup, VariantGroups,
};
use capability::{Capability, Effect, Exposure, GrantedPath, Invocation, Reach};
use gpui::{App, SharedString, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::columns::ColumnType;
use settings::history::{Change, Entry};

use super::table;

pub struct StagePatchCap;

impl Capability for StagePatchCap {
    const NAME: &'static str = "stage_patch";
    const SUMMARY: &'static str = "Stage new values for several columns of several rows, each row optionally guarded by the values you read. Adds to your earlier drafts; a cell staged again takes the newer value. Use it for splitting, merging or normalising across columns. Not for file columns: rename files with stage_renames, or relink rows with stage_relinks.";
    const GROUP: &'static str = "stage";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    const RESULT: &'static str = "staged_patch";
    type Input = StagePatch;
    type Output = StagedPatch;

    fn revision(input: &StagePatch) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StagePatch) -> Result<(), RequestError> {
        input.validate()
    }

    fn label(input: &StagePatch, _: &App) -> String {
        format!("patch of {} row(s)", input.rows.len())
    }

    fn outcome(output: &StagedPatch) -> String {
        format!(
            "{} staged in {} column(s), {} stale row(s)",
            output.accepted,
            output.columns.len(),
            output.stale.len()
        )
    }

    fn run(
        input: StagePatch,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<StagedPatch, RequestError>> {
        Task::ready(crate::agent::stage_patch(
            call.agent().unwrap_or("archivist"),
            input.rows,
            input.note,
            cx,
        ))
    }
}

pub struct ValueVariantsCap;

impl Capability for ValueVariantsCap {
    const NAME: &'static str = "value_variants";
    const SUMMARY: &'static str = "Group a column's values that look like variant spellings of each other (case, accents, punctuation, word order, near misses), with how many cells hold each. Fix them with stage_patch or stage_column_rewrite.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = ValueVariants;
    type Output = VariantGroups;

    fn validate(input: &ValueVariants) -> Result<(), RequestError> {
        match (1..=100).contains(&input.limit) {
            true => Ok(()),
            false => Err(RequestError::InvalidQueryLimit),
        }
    }

    fn label(input: &ValueVariants, _: &App) -> String {
        format!("variants in {}", input.column)
    }

    fn outcome(output: &VariantGroups) -> String {
        format!("{} group(s)", output.groups.len())
    }

    fn run(
        input: ValueVariants,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<VariantGroups, RequestError>> {
        let cells = match table(cx).and_then(|state| {
            let delegate = state.read(cx).delegate();
            let col = delegate
                .data_col(&input.column)
                .ok_or(RequestError::UnknownColumn)?;
            Ok(delegate.column_cells(col))
        }) {
            Ok(cells) => cells,
            Err(error) => return Task::ready(Err(error)),
        };
        let inline = cells.len() <= 5_000;
        let group = move || {
            let comparison = clustering::core::compare_indexed(
                cells
                    .iter()
                    .enumerate()
                    .map(|(row, text)| (row, text.as_ref())),
                &mut clustering::core::TokenCache::default(),
            );
            let mut groups: Vec<VariantGroup> =
                clustering::core::clusters(&comparison.values, &comparison.pairs)
                    .into_iter()
                    .map(|cluster| {
                        let mut values: Vec<Count> = cluster
                            .members
                            .iter()
                            .map(|value| Count {
                                value: value.displayed.clone(),
                                count: value.rows.len(),
                            })
                            .collect();
                        values.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
                        VariantGroup {
                            suggested: values[0].value.clone(),
                            values,
                            reasons: cluster.reasons.iter().map(|r| r.to_string()).collect(),
                        }
                    })
                    .collect();
            groups.sort_by_key(|group| {
                std::cmp::Reverse(group.values.iter().map(|v| v.count).sum::<usize>())
            });
            let truncated = groups.len() > input.limit;
            groups.truncate(input.limit);
            Ok(VariantGroups { groups, truncated })
        };
        match inline {
            true => Task::ready(group()),
            false => cx.background_executor().spawn(async move { group() }),
        }
    }
}

pub struct HistoryCap;

impl Capability for HistoryCap {
    const NAME: &'static str = "history";
    const SUMMARY: &'static str = "Read the project's History, newest first: who changed what and how, including your own credited changes. Filter to one row to see how it came to be.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = HistoryQuery;
    type Output = HistoryPage;

    fn validate(input: &HistoryQuery) -> Result<(), RequestError> {
        match (1..=ai::agent::MAX_QUERY_LIMIT).contains(&input.limit) {
            true => Ok(()),
            false => Err(RequestError::InvalidQueryLimit),
        }
    }

    fn label(input: &HistoryQuery, _: &App) -> String {
        match input.row {
            Some(row) => format!("history of row {}", row + 1),
            None => "history".into(),
        }
    }

    fn outcome(output: &HistoryPage) -> String {
        format!("{} entries", output.entries.len())
    }

    fn run(
        input: HistoryQuery,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<HistoryPage, RequestError>> {
        Task::ready((|| {
            let project = cx
                .try_global::<settings::project::CurrentProject>()
                .ok_or(RequestError::ProjectUnavailable)?;
            let state = table(cx)?;
            let delegate = state.read(cx).delegate();
            let row_id = match input.row {
                Some(row) => Some(delegate.row_id(row).ok_or(RequestError::RowOutOfRange)?),
                None => None,
            };
            let touches = |entry: &Entry| {
                row_id.is_none_or(|id| {
                    entry
                        .changes
                        .iter()
                        .any(|change| change_row(change) == Some(id))
                })
            };
            let row_of = |id: settings::project::RowId| delegate.row_of(id).map(|row| row + 1);
            let mut entries = Vec::new();
            if input.before.is_none() {
                let unsaved: Vec<&Entry> = delegate
                    .unsaved_history()
                    .iter()
                    .rev()
                    .filter(|entry| touches(entry))
                    .take(input.limit)
                    .collect();
                let times = settings::history::local_times(
                    &unsaved.iter().map(|entry| entry.at).collect::<Vec<_>>(),
                );
                for (entry, (day, time)) in unsaved.into_iter().zip(times) {
                    entries.push(item(entry, format!("{day} {time}"), &row_of));
                }
            }
            let mut next_before = None;
            let room = input.limit - entries.len();
            if room > 0 {
                let before = input.before.map_or(i64::MAX, |before| before as i64);
                let page = settings::history::page(&project.file, before, room as i64, row_id)
                    .unwrap_or_else(|error| {
                        log::warn!("an agent's History read failed: {error}");
                        Vec::new()
                    });
                if page.len() == room {
                    next_before = page.last().map(|listed| listed.entry.id as u64);
                }
                for listed in &page {
                    entries.push(item(
                        &listed.entry,
                        format!("{} {}", listed.day, listed.time),
                        &row_of,
                    ));
                }
            }
            Ok(HistoryPage {
                entries,
                next_before,
            })
        })())
    }
}

fn change_row(change: &Change) -> Option<settings::project::RowId> {
    match change {
        Change::Cell { row, .. }
        | Change::RowAdded { row, .. }
        | Change::RowRemoved { row, .. }
        | Change::RowArranged { row, .. } => Some(*row),
        Change::Note { row, .. } => *row,
        _ => None,
    }
}

fn item(
    entry: &Entry,
    at: String,
    row_of: &dyn Fn(settings::project::RowId) -> Option<usize>,
) -> HistoryItem {
    let short = |text: &str| match text.chars().count() > 80 {
        true => format!("{}…", text.chars().take(80).collect::<String>()),
        false => text.to_owned(),
    };
    let row = |id: settings::project::RowId| match row_of(id) {
        Some(row) => format!("row {row}"),
        None => "a removed row".into(),
    };
    let changes = entry
        .changes
        .iter()
        .take(20)
        .map(|change| match change {
            Change::Cell {
                row: id,
                column,
                before,
                after,
            } => format!(
                "{column} {}: \"{}\" → \"{}\"",
                row(*id),
                short(before),
                short(after)
            ),
            Change::RowAdded { row: id, .. } => format!("added {}", row(*id)),
            Change::RowRemoved { position, .. } => format!("removed the row at {}", position + 1),
            Change::ColumnAdded { column, .. } => format!("added column {column}"),
            Change::ColumnRemoved { column, .. } => format!("removed column {column}"),
            Change::ColumnRenamed { before, after } => {
                format!("renamed column {before} to {after}")
            }
            Change::ColumnMoved { column, .. } => format!("moved column {column}"),
            Change::RowArranged { row: id, .. } => format!("moved {}", row(*id)),
            Change::ColumnDescribed { column, .. } => format!("described column {column}"),
            Change::ColumnFiltered { column, .. } => format!("filtered column {column}"),
            Change::Note {
                row: id,
                column,
                after,
                ..
            } => {
                let at = match (id.map(row), column) {
                    (Some(row), Some(column)) => format!("{column} {row}"),
                    (Some(row), None) => row,
                    (None, Some(column)) => format!("column {column}"),
                    (None, None) => "the project".into(),
                };
                match after {
                    Some(text) => format!("note on {at}: \"{}\"", short(text)),
                    None => format!("removed the note on {at}"),
                }
            }
        })
        .chain(
            (entry.changes.len() > 20).then(|| format!("… and {} more", entry.changes.len() - 20)),
        )
        .collect();
    HistoryItem {
        id: entry.id as u64,
        at,
        origin: entry.origin.label(),
        credit: settings::history::credit(entry.author.as_deref(), &entry.origin),
        changes,
    }
}

pub struct NotesCap;

impl Capability for NotesCap {
    const NAME: &'static str = "notes";
    const SUMMARY: &'static str = "Read the notes on cells and rows, with who wrote them and when.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = NotesQuery;
    type Output = NotesPage;

    fn label(input: &NotesQuery, _: &App) -> String {
        match input.rows.len() {
            0 => "notes".into(),
            n => format!("notes on {n} row(s)"),
        }
    }

    fn outcome(output: &NotesPage) -> String {
        format!("{} note(s)", output.notes.len())
    }

    fn run(
        input: NotesQuery,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<NotesPage, RequestError>> {
        let notes = diagnostics::Diagnostics::all(cx)
            .iter()
            .filter(|note| {
                note.source == diagnostics::Source::Note
                    && note.location.dataset == diagnostics::DATASET_MAIN
                    && (input.rows.is_empty()
                        || note
                            .location
                            .row
                            .is_some_and(|row| input.rows.contains(&row)))
            })
            .take(200)
            .map(|note| NoteItem {
                row: note.location.row,
                column: note.location.column.as_ref().map(ToString::to_string),
                text: note.message.to_string(),
                author: note
                    .filed
                    .as_ref()
                    .and_then(|filed| filed.author.as_ref().map(ToString::to_string)),
                date: note
                    .filed
                    .as_ref()
                    .and_then(|filed| filed.date.as_ref().map(ToString::to_string)),
            })
            .collect();
        Task::ready(Ok(NotesPage { notes }))
    }
}

pub struct AddNoteCap;

impl AddNoteCap {
    /// An agent's notes start with its name, so the archivist sees whose they are and an agent can
    /// only ever replace its own.
    fn signed(agent: &str, text: &str) -> String {
        format!("{agent}: {}", text.trim())
    }
}

impl Capability for AddNoteCap {
    const NAME: &'static str = "add_note";
    const SUMMARY: &'static str = "Attach a note to a cell or row, signed with your name, to explain a finding or ask the archivist something. Never overwrites a note someone else wrote.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Edit;
    const RESULT: &'static str = "edited";
    type Input = AddNote;
    type Output = Edited;

    fn revision(input: &AddNote) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &AddNote) -> Result<(), RequestError> {
        match input.text.len() > ai::agent::MAX_NOTE_LEN {
            true => Err(RequestError::NoteTooLong),
            false => Ok(()),
        }
    }

    fn label(input: &AddNote, _: &App) -> String {
        match (&input.column, input.text.trim().is_empty()) {
            (_, true) => format!("Remove a note on row {}", input.row + 1),
            (Some(column), false) => format!("Note on {column} row {}", input.row + 1),
            (None, false) => format!("Note on row {}", input.row + 1),
        }
    }

    fn outcome(output: &Edited) -> String {
        output.detail.clone()
    }

    fn run(input: AddNote, call: &Invocation, cx: &mut App) -> Task<Result<Edited, RequestError>> {
        Task::ready((|| {
            let state = table(cx)?;
            let delegate = state.read(cx).delegate();
            let row_id = delegate
                .row_id(input.row)
                .ok_or(RequestError::RowOutOfRange)?;
            if let Some(column) = &input.column {
                delegate
                    .data_col(column)
                    .ok_or(RequestError::UnknownColumn)?;
            }
            let location = diagnostics::Location {
                dataset: diagnostics::DATASET_MAIN.into(),
                row: Some(input.row),
                row_id: Some(row_id),
                column: input.column.clone().map(SharedString::from),
            };
            let agent = call.agent().unwrap_or("archivist");
            let mine = format!("{agent}: ");
            let existing = diagnostics::Diagnostics::notes_at(
                diagnostics::DATASET_MAIN,
                Some(input.row),
                input.column.as_deref(),
                cx,
            )
            .find(|note| note.location.column.as_deref() == input.column.as_deref())
            .map(|note| note.message.clone());
            if call.agent().is_some()
                && existing
                    .as_ref()
                    .is_some_and(|text| !text.starts_with(&mine))
            {
                return Err(RequestError::EditRefused(
                    "a note someone else wrote is already there".into(),
                ));
            }
            let text = match input.text.trim().is_empty() {
                true => SharedString::default(),
                false => Self::signed(agent, &input.text).into(),
            };
            diagnostics::Diagnostics::set_note(location, text, call.origin(), cx);
            Ok(Edited {
                detail: call.label.clone(),
                rows: vec![input.row],
            })
        })())
    }
}

/// Point rows at other files.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct StageRelinks {
    pub revision: TableRevision,
    /// At most 50,000.
    pub links: Vec<Relink>,
    /// Shown beside the draft.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Relink {
    pub row: usize,
    /// The file column; defaults to the row's first one.
    pub column: Option<String>,
    /// The file the row should link. It must exist.
    pub path: GrantedPath,
}

pub struct StageRelinksCap;

impl Capability for StageRelinksCap {
    const NAME: &'static str = "stage_relinks";
    const SUMMARY: &'static str = "Stage new file links for rows, such as for files that moved, added to your earlier drafts. Each file must exist and be inside the project or covered by a read grant; applying repoints the rows as one undo step.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Stage;
    const RESULT: &'static str = "staged_relinks";
    type Input = StageRelinks;
    type Output = StagedRelinks;

    fn revision(input: &StageRelinks) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &StageRelinks) -> Result<(), RequestError> {
        if input.links.len() > ai::agent::MAX_RENAMES {
            return Err(RequestError::TooManyRenames);
        }
        match input
            .note
            .as_ref()
            .is_some_and(|note| note.len() > ai::agent::MAX_MESSAGE_LEN)
        {
            true => Err(RequestError::RewriteNoteTooLong),
            false => Ok(()),
        }
    }

    fn paths(input: &mut StageRelinks) -> Vec<&mut GrantedPath> {
        input.links.iter_mut().map(|link| &mut link.path).collect()
    }

    fn label(input: &StageRelinks, _: &App) -> String {
        format!("{} relink(s)", input.links.len())
    }

    fn outcome(output: &StagedRelinks) -> String {
        format!("{} relink(s) staged", output.accepted)
    }

    fn run(
        input: StageRelinks,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<StagedRelinks, RequestError>> {
        let links: Result<Vec<_>, _> = input
            .links
            .iter()
            .map(|link| {
                Ok((
                    link.row,
                    link.column.clone(),
                    link.path.path()?.to_path_buf(),
                ))
            })
            .collect();
        Task::ready(links.and_then(|links| {
            crate::agent::stage_relinks(call.agent().unwrap_or("archivist"), links, input.note, cx)
        }))
    }
}

/// Where rows' files might be now.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct FindCandidates {
    /// Source rows to look for, at most 100. Empty means every row whose file is missing.
    #[serde(default)]
    pub rows: Vec<usize>,
    /// Folders to search, 1 to 8, each inside the project or covered by a read grant.
    pub folders: Vec<GrantedPath>,
    /// Search subfolders too. Defaults to true.
    #[serde(default = "yes")]
    pub recursive: bool,
}

fn yes() -> bool {
    true
}

/// Files a search will look at before it stops.
const MAX_SCANNED: usize = 50_000;

pub struct FindCandidatesCap;

impl Capability for FindCandidatesCap {
    const NAME: &'static str = "find_candidates";
    const SUMMARY: &'static str = "Search folders for files that could be what rows link to, by file name and then by name without extension, especially for missing files. Stage the ones you are sure of with stage_relinks.";
    const GROUP: &'static str = "files";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Reach(Reach::Files);
    type Input = FindCandidates;
    type Output = Candidates;

    fn validate(input: &FindCandidates) -> Result<(), RequestError> {
        if input.folders.is_empty() || input.folders.len() > 8 {
            return Err(RequestError::TooManyFolders);
        }
        match input.rows.len() > ai::agent::MAX_EXPLICIT_ROWS {
            true => Err(RequestError::InvalidExplicitRows),
            false => Ok(()),
        }
    }

    fn paths(input: &mut FindCandidates) -> Vec<&mut GrantedPath> {
        input.folders.iter_mut().collect()
    }

    fn label(input: &FindCandidates, _: &App) -> String {
        format!("look for files in {} folder(s)", input.folders.len())
    }

    fn outcome(output: &Candidates) -> String {
        format!(
            "{} row(s), {} files scanned",
            output.items.len(),
            output.scanned
        )
    }

    fn run(
        input: FindCandidates,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<Candidates, RequestError>> {
        let prepared = (|| {
            let folders = input
                .folders
                .iter()
                .map(|folder| folder.path().map(Path::to_path_buf))
                .collect::<Result<Vec<_>, _>>()?;
            let state = table(cx)?;
            let delegate = state.read(cx).delegate();
            let file_columns: Vec<usize> = (0..delegate.column_count())
                .filter(|col| delegate.column_type(*col) == ColumnType::Filename)
                .collect();
            let missing = crate::file_links::missing_files(delegate, cx);
            let targets: Vec<(usize, String, String, bool)> = match input.rows.is_empty() {
                true => missing
                    .iter()
                    .take(ai::agent::MAX_EXPLICIT_ROWS)
                    .map(|(row, col, value)| {
                        (
                            *row,
                            delegate.column_name(*col).to_string(),
                            value.to_string(),
                            true,
                        )
                    })
                    .collect(),
                false => input
                    .rows
                    .iter()
                    .filter_map(|row| {
                        let col = *file_columns.first()?;
                        let value = delegate.cell(*row, col)?.to_string();
                        let gone = missing.iter().any(|(r, c, _)| r == row && *c == col);
                        Some((*row, delegate.column_name(col).to_string(), value, gone))
                    })
                    .collect(),
            };
            Ok::<_, RequestError>((folders, targets))
        })();
        let (folders, targets) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Task::ready(Err(error)),
        };
        let recursive = input.recursive;
        cx.background_executor().spawn(async move {
            let mut by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
            let mut by_stem: HashMap<String, Vec<PathBuf>> = HashMap::new();
            let mut scanned = 0;
            let mut pending: Vec<PathBuf> = folders;
            while let Some(folder) = pending.pop() {
                let Ok(entries) = std::fs::read_dir(&folder) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') || file_ingest::is_ignored(&name) {
                        continue;
                    }
                    let path = entry.path();
                    match entry.file_type() {
                        Ok(kind) if kind.is_dir() && recursive => pending.push(path),
                        Ok(kind) if kind.is_file() && scanned < MAX_SCANNED => {
                            scanned += 1;
                            let stem = Path::new(&name)
                                .file_stem()
                                .map(|stem| stem.to_string_lossy().to_lowercase());
                            by_name
                                .entry(name.to_lowercase())
                                .or_default()
                                .push(path.clone());
                            if let Some(stem) = stem {
                                by_stem.entry(stem).or_default().push(path);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let items = targets
                .into_iter()
                .map(|(row, column, value, missing)| {
                    let linked = Path::new(value.trim());
                    let name = linked
                        .file_name()
                        .map(|name| name.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    let stem = linked
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    // A link without an extension names files by stem, as the files folder resolves it.
                    let by_stem_reason = match linked.extension() {
                        Some(_) => "same_stem",
                        None => "same_name",
                    };
                    let mut seen = BTreeMap::new();
                    for path in by_name.get(&name).into_iter().flatten() {
                        seen.entry(path.clone()).or_insert("same_name");
                    }
                    for path in by_stem.get(&stem).into_iter().flatten() {
                        seen.entry(path.clone()).or_insert(by_stem_reason);
                    }
                    let mut candidates: Vec<Candidate> = seen
                        .into_iter()
                        .map(|(path, reason)| Candidate {
                            path: file_ingest::normalized_path(&path),
                            reason: reason.into(),
                        })
                        .collect();
                    candidates.sort_by_key(|candidate| candidate.reason != "same_name");
                    candidates.truncate(10);
                    CandidateSet {
                        row,
                        column,
                        value,
                        missing,
                        candidates,
                    }
                })
                .collect();
            Ok(Candidates {
                items,
                scanned,
                truncated: scanned >= MAX_SCANNED,
            })
        })
    }
}

pub(super) fn register(cx: &mut App) {
    capability::register::<StagePatchCap>(cx);
    capability::register::<ValueVariantsCap>(cx);
    capability::register::<HistoryCap>(cx);
    capability::register::<NotesCap>(cx);
    capability::register::<AddNoteCap>(cx);
    capability::register::<StageRelinksCap>(cx);
    capability::register::<FindCandidatesCap>(cx);
}
