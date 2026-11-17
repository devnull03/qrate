//! The agent contract between a running qrate app and a review agent, reached through `qrate agent`.
//!
//! Reads are progressive: a cheap overview, a bounded declarative query, and optional scratch
//! programs and thumbnails. Cell values and file names are only staged as drafts — findings, a
//! whole column rewrite, file renames — that the archivist applies. An agent may change the table's
//! shape at once (add rows and columns, rename, move, describe, arrange, filter), each one undo
//! step; deleting rows or a column always waits for the archivist to confirm it in the app.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TableRevision(pub u64);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Overview,
    Query(Query),
    ProgramSave {
        source: String,
    },
    ProgramRun {
        revision: TableRevision,
        #[serde(default)]
        args: Value,
    },
    Thumbnails {
        items: Vec<ThumbnailRequest>,
    },
    StageFindings {
        revision: TableRevision,
        findings: Vec<Finding>,
    },
    /// One column's new values as `(source row, value)`, staged as a single draft.
    StageColumnRewrite {
        revision: TableRevision,
        column: String,
        values: Vec<(usize, String)>,
        /// Shown once beside the draft, such as where the values came from.
        note: Option<String>,
    },
    /// Renames of files rows link to, staged as a single draft.
    StageRenames {
        revision: TableRevision,
        renames: Vec<Rename>,
    },
    /// One change to the table's shape, made at once as one undo step.
    Edit {
        revision: TableRevision,
        #[serde(flatten)]
        edit: TableEdit,
    },
    /// Ask the archivist for read-only access to one file or directory until qrate closes.
    RequestAccess {
        path: String,
        reason: String,
    },
    /// Check an existing read grant without asking or expanding its scope.
    CheckAccess {
        path: String,
    },
    /// Revoke this agent's read grants for the current project.
    ForgetAccess {},
    /// Attach approved files and folders as new rows, skipping existing components.
    ImportFiles {
        revision: TableRevision,
        paths: Vec<String>,
        #[serde(default)]
        recursive: bool,
    },
    /// Delete rows or one column. qrate asks the archivist first, showing `reason`, and answers
    /// only once they have chosen.
    Delete {
        revision: TableRevision,
        #[serde(default)]
        rows: Vec<usize>,
        column: Option<String>,
        reason: String,
    },
}

/// A change to the table that an agent makes directly. Rows are source-row indices.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TableEdit {
    /// Append rows, each as column name → value; columns left out stay blank.
    AddRows {
        rows: Vec<BTreeMap<String, String>>,
    },
    /// A new, empty column after `after`, or first when `after` is absent.
    AddColumn {
        name: String,
        after: Option<String>,
    },
    RenameColumn {
        column: String,
        name: String,
    },
    /// Move a column after `after`, or to the front when `after` is absent.
    MoveColumn {
        column: String,
        after: Option<String>,
    },
    /// Set a column's data type, its description, or both.
    DescribeColumn {
        column: String,
        data_type: Option<String>,
        description: Option<String>,
    },
    MoveRow {
        row: usize,
        target: usize,
        placement: RowPlacement,
    },
    IndentRow {
        row: usize,
    },
    OutdentRow {
        row: usize,
    },
    DuplicateRow {
        row: usize,
    },
    /// Show only rows whose `column` holds one of `keep`, or every row when `keep` is absent.
    Filter {
        column: String,
        keep: Option<Vec<String>>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowPlacement {
    Before,
    After,
    Child,
}

/// One file to rename. Both paths are relative to the folder holding the project file.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Rename {
    /// The source row that links `from`.
    pub row: usize,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Query {
    #[serde(default)]
    pub source: QuerySource,
    #[serde(default)]
    pub select: Vec<String>,
    #[serde(default, rename = "where")]
    pub filters: Vec<QueryFilter>,
    #[serde(default)]
    pub group_by: Vec<String>,
    /// Return each exact value of one field once. Mutually exclusive with `select` and `group_by`.
    pub distinct: Option<String>,
    pub order_by: Option<OrderBy>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuerySource {
    #[default]
    AllRows,
    SelectedRows,
    Rows {
        rows: Vec<usize>,
    },
    Search {
        text: String,
    },
    Diagnostics {
        #[serde(default)]
        severities: Vec<Severity>,
        #[serde(default)]
        sources: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct QueryFilter {
    pub field: String,
    pub op: FilterOp,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OrderBy {
    pub field: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ThumbnailRequest {
    pub row: usize,
    #[serde(default)]
    pub page: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub row: usize,
    pub column: String,
    pub severity: Severity,
    pub message: String,
    pub expected: String,
    pub replacement: Option<String>,
}

impl Request {
    pub fn validate(&self) -> Result<(), RequestError> {
        match self {
            Self::RequestAccess { path, reason }
                if path.trim().is_empty()
                    || path.len() > 4096
                    || reason.trim().is_empty()
                    || reason.len() > MAX_MESSAGE_LEN =>
            {
                Err(RequestError::InvalidFileAccess)
            }
            Self::CheckAccess { path } if path.trim().is_empty() || path.len() > 4096 => {
                Err(RequestError::InvalidFileAccess)
            }
            Self::ImportFiles { paths, .. }
                if paths.is_empty()
                    || paths.len() > MAX_NEW_ROWS
                    || paths
                        .iter()
                        .any(|path| path.trim().is_empty() || path.len() > 4096) =>
            {
                Err(RequestError::InvalidFileAccess)
            }

            Self::Query(query) => query.validate(),
            Self::ProgramSave { source } if source.trim().is_empty() => {
                Err(RequestError::EmptyProgram)
            }
            Self::ProgramSave { source } if source.len() > MAX_PROGRAM_BYTES => {
                Err(RequestError::ProgramTooLarge)
            }
            Self::Thumbnails { items } if items.is_empty() => Err(RequestError::EmptyThumbnails),
            Self::Thumbnails { items } if items.len() > MAX_THUMBNAILS => {
                Err(RequestError::TooManyThumbnails)
            }
            Self::StageFindings { findings, .. } if findings.len() > MAX_FINDINGS => {
                Err(RequestError::TooManyFindings)
            }
            Self::StageFindings { findings, .. }
                if findings.iter().any(|f| f.message.trim().is_empty()) =>
            {
                Err(RequestError::EmptyFindingMessage)
            }
            Self::StageFindings { findings, .. }
                if findings.iter().any(|f| f.message.len() > MAX_MESSAGE_LEN) =>
            {
                Err(RequestError::FindingMessageTooLong)
            }
            Self::StageColumnRewrite { values, note, .. } => {
                if values.len() > MAX_REWRITE_ROWS {
                    return Err(RequestError::TooManyRewriteRows);
                }
                if values.iter().any(|(_, v)| v.len() > MAX_REWRITE_VALUE) {
                    return Err(RequestError::RewriteValueTooLarge);
                }
                if note.as_ref().is_some_and(|n| n.len() > MAX_MESSAGE_LEN) {
                    return Err(RequestError::RewriteNoteTooLong);
                }
                let mut rows = std::collections::HashSet::with_capacity(values.len());
                if !values.iter().all(|(row, _)| rows.insert(*row)) {
                    return Err(RequestError::DuplicateRewriteRow);
                }
                Ok(())
            }
            Self::StageRenames { renames, .. } if renames.len() > MAX_RENAMES => {
                Err(RequestError::TooManyRenames)
            }
            Self::Edit {
                edit: TableEdit::AddRows { rows },
                ..
            } if rows.is_empty() || rows.len() > MAX_NEW_ROWS => Err(RequestError::TooManyNewRows),
            Self::Delete { reason, .. } if reason.trim().is_empty() => {
                Err(RequestError::DeleteReasonMissing)
            }
            Self::Delete { reason, .. } if reason.len() > MAX_MESSAGE_LEN => {
                Err(RequestError::DeleteReasonTooLong)
            }
            Self::Delete { rows, column, .. }
                if rows.is_empty() == column.is_none() || rows.len() > MAX_NEW_ROWS =>
            {
                Err(RequestError::DeleteTargetUnclear)
            }
            _ => Ok(()),
        }
    }
}

impl Query {
    fn validate(&self) -> Result<(), RequestError> {
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Response {
    pub revision: TableRevision,
    #[serde(flatten)]
    pub result: ResultSet,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ResultSet {
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
    /// `unchanged` lists the source rows that already hold the value asked for.
    StagedRewrite {
        accepted: usize,
        unchanged: Vec<usize>,
    },
    StagedRenames {
        accepted: usize,
    },
    /// What the edit did, and the source rows it added or moved.
    Edited {
        detail: String,
        rows: Vec<usize>,
    },
    FileAccess {
        path: String,
        directory: bool,
    },
    FileAccessRevoked {
        count: usize,
    },
    ImportedFiles {
        added: usize,
        skipped: usize,
        rows: Vec<usize>,
        warnings: Vec<String>,
    },
    Deleted {
        count: usize,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Overview {
    pub project: ProjectSummary,
    pub columns: Vec<Column>,
    pub selected_rows: usize,
    pub diagnostics: DiagnosticSummary,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ProjectSummary {
    pub name: String,
    pub row_count: usize,
    pub column_count: usize,
    pub has_files_folder: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub notes: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct DiagnosticSummary {
    pub errors: usize,
    pub warnings: usize,
    pub notes: usize,
    pub sources: Vec<Count>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Count {
    pub value: String,
    pub count: usize,
}

/// A schema-once page. Every item uses the positions in `fields`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueryResult {
    pub fields: Vec<String>,
    pub items: Vec<Vec<Value>>,
    pub returned: usize,
    pub remaining: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProgramOutput {
    pub version: u64,
    pub hash: String,
    pub value: Value,
    pub logs: Vec<String>,
    pub elapsed_ms: u64,
    pub audit: ProgramAudit,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Thumbnail {
    pub row: usize,
    pub page: usize,
    pub media_type: String,
    pub data: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum RequestError {
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
    DeleteReasonMissing,
    DeleteReasonTooLong,
    /// A delete must name rows or one column, not both or neither.
    DeleteTargetUnclear,
    /// Another delete is already waiting for the archivist.
    DeletePending,
    DeleteDeclined,
    DeleteTimedOut,
    /// No qrate window is open to ask the archivist in.
    NoWindow,
    InvalidFileAccess,
    FileAccessRequired(String),
    FileAccessDenied,
    FileAccessPending,
    FileAccessTimedOut,
    FileAccessFailed(String),
    FileImportFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_defaults_are_small_and_bounded() {
        let request: Request =
            serde_json::from_value(serde_json::json!({"method":"query","params":{}})).unwrap();
        let Request::Query(query) = request else {
            panic!()
        };
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
            Request::Thumbnails { items: vec![] }.validate(),
            Err(RequestError::EmptyThumbnails)
        );
        assert_eq!(
            Request::ProgramSave { source: " ".into() }.validate(),
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
        let rewrite = |values: Vec<(usize, String)>| Request::StageColumnRewrite {
            revision: TableRevision(1),
            column: "Identifier".into(),
            values,
            note: None,
        };
        let request: Request = serde_json::from_value(serde_json::json!({
            "method": "stage_column_rewrite",
            "params": {"revision": 1, "column": "Identifier", "values": [[0, "a"], [1, "b"]]}
        }))
        .unwrap();
        assert_eq!(request, rewrite(vec![(0, "a".into()), (1, "b".into())]));
        assert_eq!(request.validate(), Ok(()));
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
    fn an_edit_reads_its_operation_from_the_same_object() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "method": "edit",
            "params": {"revision": 4, "op": "add_rows", "rows": [{"Title": "New"}]}
        }))
        .unwrap();
        assert_eq!(
            request,
            Request::Edit {
                revision: TableRevision(4),
                edit: TableEdit::AddRows {
                    rows: vec![BTreeMap::from([("Title".into(), "New".into())])]
                },
            }
        );
        assert_eq!(request.validate(), Ok(()));
        let request: Request = serde_json::from_value(serde_json::json!({
            "method": "edit",
            "params": {"revision": 4, "op": "move_row", "row": 2, "target": 0, "placement": "before"}
        }))
        .unwrap();
        assert!(matches!(
            request,
            Request::Edit {
                edit: TableEdit::MoveRow {
                    placement: RowPlacement::Before,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn a_delete_needs_a_reason_and_one_target() {
        let delete = |rows: Vec<usize>, column: Option<&str>, reason: &str| Request::Delete {
            revision: TableRevision(1),
            rows,
            column: column.map(Into::into),
            reason: reason.into(),
        };
        assert_eq!(
            delete(vec![1], None, "duplicate of row 0").validate(),
            Ok(())
        );
        assert_eq!(
            delete(vec![], Some("Notes"), "empty column").validate(),
            Ok(())
        );
        assert_eq!(
            delete(vec![1], None, " ").validate(),
            Err(RequestError::DeleteReasonMissing)
        );
        assert_eq!(
            delete(vec![], None, "why").validate(),
            Err(RequestError::DeleteTargetUnclear)
        );
        assert_eq!(
            delete(vec![1], Some("Notes"), "why").validate(),
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
    }

    #[test]
    fn pages_are_schema_once() {
        let value = serde_json::to_value(Response {
            revision: TableRevision(4),
            result: ResultSet::Query(QueryResult {
                fields: vec!["row".into(), "Title".into()],
                items: vec![vec![Value::from(2), Value::from("Harvest")]],
                returned: 1,
                remaining: 0,
                truncated: false,
                next_cursor: None,
            }),
        })
        .unwrap();
        assert_eq!(value["result"], "query");
        assert_eq!(value["items"][0][1], "Harvest");
    }
}
