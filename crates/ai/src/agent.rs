//! The agent contract between a running qrate app and a review agent, reached through `qrate agent`.
//!
//! These are the data each capability takes and returns; what a capability *does*, and the rules
//! every call obeys, live in the `capability` crate and the crates that implement one. Reads are
//! progressive: a cheap overview, a bounded declarative query, and optional scratch programs and
//! thumbnails. Cell values and file names are only staged as drafts the archivist applies. An agent
//! may change the table's shape at once, each change one undo step; deleting always waits for the
//! archivist to confirm it in the app.
//!
//! Doc comments on these types are the descriptions an agent reads in the generated catalog.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The table's revision. Read it from `overview`; a call that names rows or cells must pass the
/// revision it read them at, and is refused as stale if the table changed since.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct TableRevision(pub u64);

/// No parameters.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

// ---------------------------------------------------------------------------------------------
// Reads

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Query {
    /// Which records to read. Defaults to every row.
    #[serde(default)]
    pub source: QuerySource,
    /// Fields to return, in order. Empty returns every field.
    #[serde(default)]
    pub select: Vec<String>,
    #[serde(default, rename = "where")]
    pub filters: Vec<QueryFilter>,
    /// Count records per combination of these fields instead of returning them.
    #[serde(default)]
    pub group_by: Vec<String>,
    /// Return each exact value of one field once. Mutually exclusive with `select` and `group_by`.
    pub distinct: Option<String>,
    pub order_by: Option<OrderBy>,
    /// Records per page, 1 to 50.
    #[serde(default = "default_query_limit")]
    pub limit: usize,
    /// `next_cursor` from the previous page of the same query.
    pub cursor: Option<String>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            source: QuerySource::default(),
            select: Vec::new(),
            filters: Vec::new(),
            group_by: Vec::new(),
            distinct: None,
            order_by: None,
            limit: DEFAULT_QUERY_LIMIT,
            cursor: None,
        }
    }
}

fn default_query_limit() -> usize {
    DEFAULT_QUERY_LIMIT
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuerySource {
    #[default]
    AllRows,
    /// The rows the archivist has selected.
    SelectedRows,
    /// Source-row indices, at most 100.
    Rows { rows: Vec<usize> },
    /// Rows with a cell containing `text`.
    Search { text: String },
    /// The Problems panel's diagnostics instead of rows.
    Diagnostics {
        #[serde(default)]
        severities: Vec<Severity>,
        #[serde(default)]
        sources: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct QueryFilter {
    pub field: String,
    pub op: FilterOp,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    IsBlank,
    IsNotBlank,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OrderBy {
    pub field: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProgramSave {
    /// Luau source returning `function(table, args)`. No network, filesystem, process, clock,
    /// randomness, plugin storage, UI or staging access; 2 s and 16 KB of output per run.
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct ProgramRun {
    pub revision: TableRevision,
    /// Any JSON, handed to the program as `args`.
    #[serde(default)]
    pub args: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Thumbnails {
    /// At most 4.
    pub items: Vec<ThumbnailRequest>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThumbnailRequest {
    pub row: usize,
    /// Page of a multi-page file, from 0.
    #[serde(default)]
    pub page: usize,
}

// ---------------------------------------------------------------------------------------------
// Drafts

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StageFindings {
    pub revision: TableRevision,
    /// The complete batch, at most 200. Replaces this agent's previous findings.
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Finding {
    pub row: usize,
    pub column: String,
    pub severity: Severity,
    /// At most 512 bytes.
    pub message: String,
    /// The exact whole cell text that was reviewed.
    pub expected: String,
    /// A whole replacement cell text, only when well supported.
    pub replacement: Option<String>,
}

/// One column's new values, staged as a single draft the archivist previews and applies.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StageColumnRewrite {
    pub revision: TableRevision,
    pub column: String,
    /// `[source row, new value]` pairs, each row once, at most 50,000.
    pub values: Vec<(usize, String)>,
    /// Shown once beside the draft, such as where the values came from.
    pub note: Option<String>,
}

/// Renames of files rows link to, staged as a single draft.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StageRenames {
    pub revision: TableRevision,
    pub renames: Vec<Rename>,
}

/// One file to rename. Both paths are relative to the folder holding the project file.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Rename {
    /// The source row that links `from`.
    pub row: usize,
    pub from: String,
    pub to: String,
}

/// Several columns of several rows, staged as one draft. A column it touches replaces this agent's
/// earlier draft for that column.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StagePatch {
    pub revision: TableRevision,
    /// Each source row at most once; 50,000 cells in all.
    pub rows: Vec<PatchRow>,
    /// Shown beside the draft, such as where the values came from.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PatchRow {
    pub row: usize,
    /// Column name → new whole-cell value.
    pub set: BTreeMap<String, String>,
    /// Column name → the whole-cell value you read. A row whose cells no longer match is skipped
    /// as stale rather than staged.
    #[serde(default)]
    pub expect: BTreeMap<String, String>,
}

impl StagePatch {
    pub fn validate(&self) -> Result<(), RequestError> {
        let cells: usize = self.rows.iter().map(|row| row.set.len()).sum();
        if cells > MAX_REWRITE_ROWS {
            return Err(RequestError::TooManyRewriteRows);
        }
        if self
            .rows
            .iter()
            .flat_map(|row| row.set.values())
            .any(|value| value.len() > MAX_REWRITE_VALUE)
        {
            return Err(RequestError::RewriteValueTooLarge);
        }
        if self
            .note
            .as_ref()
            .is_some_and(|n| n.len() > MAX_MESSAGE_LEN)
        {
            return Err(RequestError::RewriteNoteTooLong);
        }
        let mut rows = std::collections::HashSet::with_capacity(self.rows.len());
        if !self.rows.iter().all(|row| rows.insert(row.row)) {
            return Err(RequestError::DuplicateRewriteRow);
        }
        Ok(())
    }
}

/// Groups of values in one column that look like variants of each other.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ValueVariants {
    pub column: String,
    /// Groups to return, 1 to 100. Defaults to 20.
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

/// A page of the project's History, newest first.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HistoryQuery {
    /// Entries to return, 1 to 50. Defaults to 20.
    #[serde(default = "default_query_limit")]
    pub limit: usize,
    /// `next_before` from the previous page.
    pub before: Option<u64>,
    /// Only changes to this source row.
    pub row: Option<usize>,
}

/// Notes on cells and rows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NotesQuery {
    /// Only notes on these source rows.
    #[serde(default)]
    pub rows: Vec<usize>,
}

/// Attach a note to a cell, or to a whole row when `column` is absent. Replaces your own earlier
/// note there; an empty `text` removes it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AddNote {
    pub revision: TableRevision,
    pub row: usize,
    pub column: Option<String>,
    /// At most 2,000 bytes.
    pub text: String,
}

pub const MAX_NOTE_LEN: usize = 2_000;

// ---------------------------------------------------------------------------------------------
// Shape edits

/// Append rows, each as column name → value; columns left out stay blank. At most 1,000.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AddRows {
    pub revision: TableRevision,
    pub rows: Vec<BTreeMap<String, String>>,
}

/// A new, empty column after `after`, or first when `after` is absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AddColumn {
    pub revision: TableRevision,
    pub name: String,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RenameColumn {
    pub revision: TableRevision,
    pub column: String,
    pub name: String,
}

/// Move a column after `after`, or to the front when `after` is absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MoveColumn {
    pub revision: TableRevision,
    pub column: String,
    pub after: Option<String>,
}

/// Set a column's data type, its description, or both.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DescribeColumn {
    pub revision: TableRevision,
    pub column: String,
    pub data_type: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MoveRow {
    pub revision: TableRevision,
    pub row: usize,
    pub target: usize,
    pub placement: RowPlacement,
}

/// One source row.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RowEdit {
    pub revision: TableRevision,
    pub row: usize,
}

/// Show only rows whose `column` holds one of `keep`, or every row when `keep` is absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Filter {
    pub revision: TableRevision,
    pub column: String,
    pub keep: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowPlacement {
    Before,
    After,
    Child,
}

/// Delete rows or one column. qrate asks the archivist first, showing `reason`, and answers only
/// once they have chosen.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Delete {
    pub revision: TableRevision,
    /// Source rows, at most 1,000. Give rows or a column, not both.
    #[serde(default)]
    pub rows: Vec<usize>,
    pub column: Option<String>,
    /// Shown to the archivist. At most 512 bytes.
    pub reason: String,
}

// ---------------------------------------------------------------------------------------------
// Files

/// Ask the archivist for read-only access to one file or folder until qrate closes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RequestAccess {
    pub path: String,
    /// Shown to the archivist. At most 512 bytes.
    pub reason: String,
}

/// Check an existing read grant without asking or expanding its scope.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CheckAccess {
    pub path: String,
}

/// Attach approved files and folders as new rows, skipping existing components.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ImportFiles {
    pub revision: TableRevision,
    pub paths: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
}

// ---------------------------------------------------------------------------------------------
// Validation

impl Query {
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            return Err(RequestError::InvalidQueryLimit);
        }
        if self.select.len() > MAX_QUERY_FIELDS || self.group_by.len() > MAX_GROUP_FIELDS {
            return Err(RequestError::TooManyQueryFields);
        }
        if self.distinct.is_some() && (!self.select.is_empty() || !self.group_by.is_empty()) {
            return Err(RequestError::ConflictingQueryOperations);
        }
        if self.filters.len() > MAX_QUERY_FILTERS {
            return Err(RequestError::TooManyQueryFilters);
        }
        if let QuerySource::Rows { rows } = &self.source
            && (rows.is_empty() || rows.len() > MAX_EXPLICIT_ROWS)
        {
            return Err(RequestError::InvalidExplicitRows);
        }
        if let QuerySource::Search { text } = &self.source
            && (text.trim().is_empty() || text.len() > MAX_QUERY_TEXT)
        {
            return Err(RequestError::InvalidSearchText);
        }
        Ok(())
    }
}

impl ProgramSave {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.source.len() {
            _ if self.source.trim().is_empty() => Err(RequestError::EmptyProgram),
            len if len > MAX_PROGRAM_BYTES => Err(RequestError::ProgramTooLarge),
            _ => Ok(()),
        }
    }
}

impl Thumbnails {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.items.len() {
            0 => Err(RequestError::EmptyThumbnails),
            n if n > MAX_THUMBNAILS => Err(RequestError::TooManyThumbnails),
            _ => Ok(()),
        }
    }
}

impl StageFindings {
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.findings.len() > MAX_FINDINGS {
            return Err(RequestError::TooManyFindings);
        }
        if self.findings.iter().any(|f| f.message.trim().is_empty()) {
            return Err(RequestError::EmptyFindingMessage);
        }
        if self
            .findings
            .iter()
            .any(|f| f.message.len() > MAX_MESSAGE_LEN)
        {
            return Err(RequestError::FindingMessageTooLong);
        }
        Ok(())
    }
}

impl StageColumnRewrite {
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.values.len() > MAX_REWRITE_ROWS {
            return Err(RequestError::TooManyRewriteRows);
        }
        if self.values.iter().any(|(_, v)| v.len() > MAX_REWRITE_VALUE) {
            return Err(RequestError::RewriteValueTooLarge);
        }
        if self
            .note
            .as_ref()
            .is_some_and(|n| n.len() > MAX_MESSAGE_LEN)
        {
            return Err(RequestError::RewriteNoteTooLong);
        }
        let mut rows = std::collections::HashSet::with_capacity(self.values.len());
        if !self.values.iter().all(|(row, _)| rows.insert(*row)) {
            return Err(RequestError::DuplicateRewriteRow);
        }
        Ok(())
    }
}

impl StageRenames {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.renames.len() > MAX_RENAMES {
            true => Err(RequestError::TooManyRenames),
            false => Ok(()),
        }
    }
}

impl AddRows {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.rows.is_empty() || self.rows.len() > MAX_NEW_ROWS {
            true => Err(RequestError::TooManyNewRows),
            false => Ok(()),
        }
    }
}

impl Delete {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.rows.is_empty() == self.column.is_none() || self.rows.len() > MAX_NEW_ROWS {
            true => Err(RequestError::DeleteTargetUnclear),
            false => Ok(()),
        }
    }
}

fn valid_path(path: &str) -> bool {
    !path.trim().is_empty() && path.len() <= 4096
}

impl RequestAccess {
    pub fn validate(&self) -> Result<(), RequestError> {
        match valid_path(&self.path) {
            true => Ok(()),
            false => Err(RequestError::InvalidFileAccess),
        }
    }
}

impl CheckAccess {
    pub fn validate(&self) -> Result<(), RequestError> {
        match valid_path(&self.path) {
            true => Ok(()),
            false => Err(RequestError::InvalidFileAccess),
        }
    }
}

impl ImportFiles {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self.paths.is_empty()
            || self.paths.len() > MAX_NEW_ROWS
            || !self.paths.iter().all(|path| valid_path(path))
        {
            true => Err(RequestError::InvalidFileAccess),
            false => Ok(()),
        }
    }
}

pub const DEFAULT_QUERY_LIMIT: usize = 20;
pub const MAX_QUERY_LIMIT: usize = 50;
pub const MAX_QUERY_FIELDS: usize = 64;
pub const MAX_GROUP_FIELDS: usize = 4;
pub const MAX_QUERY_FILTERS: usize = 16;
pub const MAX_EXPLICIT_ROWS: usize = 100;
pub const MAX_QUERY_TEXT: usize = 256;
pub const MAX_PROGRAM_BYTES: usize = 64 * 1024;
pub const MAX_THUMBNAILS: usize = 4;
pub const MAX_FINDINGS: usize = 200;
pub const MAX_MESSAGE_LEN: usize = 512;
pub const MAX_REWRITE_ROWS: usize = 50_000;
pub const MAX_REWRITE_VALUE: usize = 64 * 1024;
pub const MAX_RENAMES: usize = 50_000;
/// Rows one `add_rows` may append, and one `delete` may remove.
pub const MAX_NEW_ROWS: usize = 1_000;
pub const MAX_RESULT_BYTES: usize = 16 * 1024;

// ---------------------------------------------------------------------------------------------
// Results

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Overview {
    pub project: ProjectSummary,
    pub columns: Vec<Column>,
    pub selected_rows: usize,
    pub diagnostics: DiagnosticSummary,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProjectSummary {
    pub name: String,
    pub row_count: usize,
    pub column_count: usize,
    pub has_files_folder: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub notes: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DiagnosticSummary {
    pub errors: usize,
    pub warnings: usize,
    pub notes: usize,
    pub sources: Vec<Count>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Count {
    pub value: String,
    pub count: usize,
}

/// A schema-once page. Every item uses the positions in `fields`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct QueryResult {
    pub fields: Vec<String>,
    pub items: Vec<Vec<Value>>,
    pub returned: usize,
    pub remaining: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProgramSaved {
    pub version: u64,
    pub hash: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct ProgramOutput {
    pub version: u64,
    pub hash: String,
    pub value: Value,
    pub logs: Vec<String>,
    pub elapsed_ms: u64,
    pub audit: ProgramAudit,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProgramAudit {
    pub run_id: String,
    pub revision: TableRevision,
    pub version: u64,
    pub hash: String,
    pub elapsed_ms: u64,
    pub memory_limit_bytes: usize,
    pub deadline_ms: u64,
    pub output_limit_bytes: usize,
    pub status: String,
    pub output_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThumbnailSet {
    pub items: Vec<Thumbnail>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Thumbnail {
    pub row: usize,
    pub page: usize,
    pub media_type: String,
    /// Base64 image data.
    pub data: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Staged {
    pub accepted: usize,
    /// Indices into `findings` whose cell no longer holds `expected`.
    pub stale: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StagedRewrite {
    pub accepted: usize,
    /// Source rows that already hold the value asked for.
    pub unchanged: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StagedPatch {
    /// Cells staged.
    pub accepted: usize,
    /// Cells that already hold the value asked for.
    pub unchanged: usize,
    /// Source rows skipped because their `expect` no longer matches.
    pub stale: Vec<usize>,
    /// The columns the draft touches.
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct VariantGroups {
    pub groups: Vec<VariantGroup>,
    /// More groups exist than `limit` allowed.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct VariantGroup {
    /// The most used spelling in the group: a suggestion, not a verdict.
    pub suggested: String,
    /// Each spelling and how many cells hold it.
    pub values: Vec<Count>,
    /// Why the values were grouped, such as "case" or "accents".
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HistoryPage {
    pub entries: Vec<HistoryItem>,
    /// Pass as `before` for the next page; absent on the last page.
    pub next_before: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HistoryItem {
    /// `0` for a change not saved yet.
    pub id: u64,
    /// `YYYY-MM-DD HH:MM`, local time.
    pub at: String,
    /// What made the change, such as "Typed" or an agent's label.
    pub origin: String,
    /// Who: the archivist, or "archivist using agent".
    pub credit: Option<String>,
    /// One line per change, such as `Title row 4: "Harvest" → "Harvest Festival"`.
    pub changes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NotesPage {
    pub notes: Vec<NoteItem>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NoteItem {
    pub row: Option<usize>,
    pub column: Option<String>,
    pub text: String,
    pub author: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StagedRelinks {
    pub accepted: usize,
    /// Rows already linking the file asked for.
    pub unchanged: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Candidates {
    pub items: Vec<CandidateSet>,
    /// Files looked at across the folders.
    pub scanned: usize,
    /// The scan stopped at its limit before seeing every file.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CandidateSet {
    pub row: usize,
    pub column: String,
    /// The link the cell holds now.
    pub value: String,
    /// Whether the linked file can be found.
    pub missing: bool,
    pub candidates: Vec<Candidate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Candidate {
    pub path: String,
    /// `same_name` or `same_stem`.
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StagedRenames {
    pub accepted: usize,
}

/// What an edit did, and the source rows it added or moved.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Edited {
    pub detail: String,
    pub rows: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileAccess {
    pub path: String,
    pub directory: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileAccessRevoked {
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ImportedFiles {
    pub added: usize,
    pub skipped: usize,
    pub rows: Vec<usize>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Deleted {
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// Why a call was refused. `code` is stable and documented; `detail`, when present, is bounded
/// text for a person.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum RequestError {
    /// The parameters do not match the capability's schema.
    MalformedParams(String),
    UnknownCapability(String),
    /// Only the archivist may use this capability.
    ArchivistOnly(String),
    /// Too many changes in the last minute; wait and try again.
    RateLimited,
    ProjectUnavailable,
    TableUnavailable,
    InvalidQueryLimit,
    TooManyQueryFields,
    TooManyQueryFilters,
    InvalidExplicitRows,
    InvalidSearchText,
    InvalidCursor,
    StaleCursor,
    UnknownField,
    ConflictingQueryOperations,
    ResultItemTooLarge,
    EmptyProgram,
    ProgramTooLarge,
    ProgramUnavailable,
    ProgramCompileFailed(String),
    ProgramFailed(String),
    ProgramOutputTooLarge(String),
    EmptyThumbnails,
    TooManyThumbnails,
    ThumbnailUnavailable,
    TooManyFindings,
    EmptyFindingMessage,
    FindingMessageTooLong,
    StaleRevision,
    UnknownColumn,
    TooManyRewriteRows,
    RewriteValueTooLarge,
    RewriteNoteTooLong,
    DuplicateRewriteRow,
    RewriteRowOutOfRange,
    TooManyRenames,
    /// A path that is absolute or leaves the project folder.
    RenameOutsideProject(String),
    /// A `from` that is not a file, or not the file its row links.
    RenameSourceMismatch(String),
    /// A `to` or a `from` named by two renames.
    RenameTargetCollision(String),
    /// A `to` that already exists and is not itself being renamed away.
    RenameTargetExists(String),
    TooManyNewRows,
    RowOutOfRange,
    /// A column name that is blank or already used.
    ColumnNameTaken(String),
    UnknownDataType(String),
    /// The table refused the change, such as indenting a row that has nothing above it.
    EditRefused(String),
    /// A change the archivist must confirm needs a reason to show them.
    ReasonMissing,
    ReasonTooLong,
    /// A delete must name rows or one column, not both or neither.
    DeleteTargetUnclear,
    /// Another request is already waiting for the archivist to confirm it.
    ConfirmPending,
    /// The archivist declined. Do not ask again for the same thing.
    Declined,
    ConfirmTimedOut,
    /// No qrate window is open to ask the archivist in.
    NoWindow,
    InvalidFileAccess,
    /// The path is outside the project and no read grant covers it. Ask with `request_access`.
    FileAccessRequired(String),
    FileAccessDenied,
    FileAccessPending,
    FileAccessTimedOut,
    FileAccessFailed(String),
    FileImportFailed(String),
    NoteTooLong,
    /// A draft that retypes a file column. Use `stage_renames` to rename the files, or
    /// `stage_relinks` to point the rows at other files.
    FileColumn(String),
    /// Visual search is off or still indexing; the archivist turns it on from the search bar.
    VisualSearchOff,
    /// The path is not a file.
    NotAFile(String),
    TooManyFolders,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = serde_json::to_value(self).unwrap_or_default();
        let code = value["code"].as_str().unwrap_or("request_error");
        match value["detail"].as_str() {
            Some(detail) => write!(f, "{code}: {detail}"),
            None => write!(f, "{code}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_defaults_are_small_and_bounded() {
        let query: Query = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(query.limit, DEFAULT_QUERY_LIMIT);
        assert_eq!(query.source, QuerySource::AllRows);
        assert_eq!(query.validate(), Ok(()));
        assert_eq!(
            Query {
                limit: MAX_QUERY_LIMIT + 1,
                ..query
            }
            .validate(),
            Err(RequestError::InvalidQueryLimit)
        );
    }

    #[test]
    fn dynamic_inputs_are_bounded() {
        assert_eq!(
            Thumbnails { items: vec![] }.validate(),
            Err(RequestError::EmptyThumbnails)
        );
        assert_eq!(
            ProgramSave { source: " ".into() }.validate(),
            Err(RequestError::EmptyProgram)
        );
        assert_eq!(
            Query {
                distinct: Some("Title".into()),
                select: vec!["Title".into()],
                ..Query::default()
            }
            .validate(),
            Err(RequestError::ConflictingQueryOperations)
        );
    }

    #[test]
    fn a_column_rewrite_is_bounded_and_names_each_row_once() {
        let rewrite = |values: Vec<(usize, String)>| StageColumnRewrite {
            revision: TableRevision(1),
            column: "Identifier".into(),
            values,
            note: None,
        };
        let parsed: StageColumnRewrite = serde_json::from_value(serde_json::json!(
            {"revision": 1, "column": "Identifier", "values": [[0, "a"], [1, "b"]]}
        ))
        .unwrap();
        assert_eq!(parsed, rewrite(vec![(0, "a".into()), (1, "b".into())]));
        assert_eq!(parsed.validate(), Ok(()));
        assert_eq!(
            rewrite(vec![(4, "a".into()), (4, "b".into())]).validate(),
            Err(RequestError::DuplicateRewriteRow)
        );
        assert_eq!(
            rewrite(
                (0..=MAX_REWRITE_ROWS)
                    .map(|row| (row, String::new()))
                    .collect()
            )
            .validate(),
            Err(RequestError::TooManyRewriteRows)
        );
        assert_eq!(
            rewrite(vec![(0, "x".repeat(MAX_REWRITE_VALUE + 1))]).validate(),
            Err(RequestError::RewriteValueTooLarge)
        );
    }

    #[test]
    fn a_delete_names_one_target() {
        let delete = |rows: Vec<usize>, column: Option<&str>| Delete {
            revision: TableRevision(1),
            rows,
            column: column.map(Into::into),
            reason: "why".into(),
        };
        assert_eq!(delete(vec![1], None).validate(), Ok(()));
        assert_eq!(delete(vec![], Some("Notes")).validate(), Ok(()));
        assert_eq!(
            delete(vec![], None).validate(),
            Err(RequestError::DeleteTargetUnclear)
        );
        assert_eq!(
            delete(vec![1], Some("Notes")).validate(),
            Err(RequestError::DeleteTargetUnclear)
        );
    }

    #[test]
    fn errors_have_a_stable_code_and_optional_bounded_detail() {
        let value = serde_json::to_value(RequestError::ProgramCompileFailed(
            "line 1: expected function".into(),
        ))
        .unwrap();
        assert_eq!(value["code"], "program_compile_failed");
        assert_eq!(value["detail"], "line 1: expected function");
        assert_eq!(
            RequestError::StaleRevision.to_string(),
            "stale_revision".to_string()
        );
    }
}
