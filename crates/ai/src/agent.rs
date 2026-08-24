//! Protocol v2 between a running qrate app and an external review agent.
//!
//! Reads are progressive: a cheap overview, a bounded declarative query, and optional scratch
//! programs and thumbnails. The only state-changing request publishes draft findings; it never
//! changes a table cell.

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
    ProgramSaved { version: u64, hash: String },
    ProgramRun(ProgramOutput),
    Thumbnails { items: Vec<Thumbnail> },
    Staged { accepted: usize, stale: Vec<usize> },
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
