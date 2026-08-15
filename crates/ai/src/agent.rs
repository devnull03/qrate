//! The narrow, read-only contract between a running qrate app and an external agent.
//!
//! This module deliberately knows nothing about MCP, GPUI, or a model provider. A transport and
//! the app-side adapter can both depend on these serializable messages without the `ai` crate
//! depending back on the table or UI.

use serde::{Deserialize, Serialize};

/// An agent-visible table revision. A later state-changing request must echo this value so qrate
/// can reject a proposal made against stale cell contents.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TableRevision(pub u64);

/// The only read operations an external agent may request in the initial bridge.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    ProjectSummary,
    Columns,
    Rows { rows: Vec<usize> },
    SearchRows { query: String, limit: usize },
    Diagnostics,
    SelectedRows,
}

impl Request {
    /// Reject oversized or ambiguous requests before the app reads live project data.
    pub fn validate(&self) -> Result<(), RequestError> {
        match self {
            Self::Rows { rows } if rows.is_empty() => Err(RequestError::EmptyRows),
            Self::Rows { rows } if rows.len() > MAX_ROWS_PER_REQUEST => {
                Err(RequestError::TooManyRows)
            }
            Self::SearchRows { query, .. } if query.trim().is_empty() => {
                Err(RequestError::EmptyQuery)
            }
            Self::SearchRows { query, .. } if query.len() > MAX_QUERY_LEN => {
                Err(RequestError::QueryTooLong)
            }
            Self::SearchRows { limit, .. } if *limit == 0 || *limit > MAX_ROWS_PER_REQUEST => {
                Err(RequestError::InvalidSearchLimit)
            }
            _ => Ok(()),
        }
    }
}

pub const MAX_ROWS_PER_REQUEST: usize = 100;
pub const MAX_QUERY_LEN: usize = 256;

/// A successful read response. Every result carries the revision used to produce it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Response {
    pub revision: TableRevision,
    #[serde(flatten)]
    pub result: ResultSet,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ResultSet {
    ProjectSummary(ProjectSummary),
    Columns { columns: Vec<Column> },
    Rows { rows: Vec<Row> },
    Diagnostics { diagnostics: Vec<Diagnostic> },
    SelectedRows { rows: Vec<usize> },
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Source-row index, never a filtered-view index.
    pub index: usize,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Field {
    pub column: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub row: Option<usize>,
    pub column: Option<String>,
    pub severity: Severity,
    pub source: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// Errors that may be returned without reading or exposing project data.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestError {
    ProjectUnavailable,
    TableUnavailable,
    EmptyRows,
    TooManyRows,
    EmptyQuery,
    QueryTooLong,
    InvalidSearchLimit,
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_QUERY_LEN, MAX_ROWS_PER_REQUEST, Request, RequestError, ResultSet, TableRevision,
    };

    #[test]
    fn request_validation_bounds_agent_reads() {
        assert_eq!(
            Request::Rows { rows: vec![] }.validate(),
            Err(RequestError::EmptyRows)
        );
        assert_eq!(
            Request::Rows {
                rows: vec![0; MAX_ROWS_PER_REQUEST + 1]
            }
            .validate(),
            Err(RequestError::TooManyRows)
        );
        assert_eq!(
            Request::SearchRows {
                query: " ".into(),
                limit: 1
            }
            .validate(),
            Err(RequestError::EmptyQuery)
        );
        assert_eq!(
            Request::SearchRows {
                query: "x".repeat(MAX_QUERY_LEN + 1),
                limit: 1
            }
            .validate(),
            Err(RequestError::QueryTooLong)
        );
        assert_eq!(
            Request::SearchRows {
                query: "title".into(),
                limit: 0
            }
            .validate(),
            Err(RequestError::InvalidSearchLimit)
        );
    }

    #[test]
    fn protocol_tags_preserve_the_requested_operation() {
        let request = serde_json::to_value(Request::SelectedRows).unwrap();
        assert_eq!(request["method"], "selected_rows");

        let response = serde_json::to_value(super::Response {
            revision: TableRevision(4),
            result: ResultSet::SelectedRows { rows: vec![1, 3] },
        })
        .unwrap();
        assert_eq!(response["revision"], 4);
        assert_eq!(response["result"], "selected_rows");
    }
}
