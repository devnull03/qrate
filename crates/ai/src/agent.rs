//! The narrow contract between a running qrate app and an external agent: reads of live project
//! state, and one call that hands findings back as drafts without changing a cell.
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

/// What an external agent may ask of the bridge. Every variant but [`Request::StageFindings`] is
/// a read; that one publishes advisory diagnostics and still changes no cell.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    ProjectSummary,
    Columns,
    Rows {
        rows: Vec<usize>,
    },
    SearchRows {
        query: String,
        limit: usize,
    },
    Diagnostics,
    SelectedRows,
    /// Hand back what a review found, as drafts. `revision` is the table the batch was judged
    /// against, echoed from the read that produced it.
    StageFindings {
        revision: TableRevision,
        findings: Vec<Finding>,
    },
}

/// One draft finding: where it is, why, and — optionally — what the agent proposes the cell should
/// say instead.
///
/// A proposal is never applied by staging it. `expected` is the cell text the agent judged, and a
/// draft is only ever offered against a cell that still says exactly that, so an archivist who
/// edited the cell in the meantime is not offered a correction to text nobody checked.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Source-row index, as handed out by [`Request::Rows`] — never a filtered-view index.
    pub row: usize,
    pub column: String,
    pub severity: Severity,
    /// Why this is a finding, in the archivist's terms. This is what the Problems panel shows.
    pub message: String,
    pub expected: String,
    /// The cell's whole new text, not a fragment. `None` is an observation with no fix to offer.
    pub replacement: Option<String>,
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

pub const MAX_ROWS_PER_REQUEST: usize = 100;
pub const MAX_QUERY_LEN: usize = 256;
pub const MAX_FINDINGS: usize = 200;
/// A finding is a sentence for the Problems panel, not an essay — and the panel's row is one line.
pub const MAX_MESSAGE_LEN: usize = 512;

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
    Columns {
        columns: Vec<Column>,
    },
    Rows {
        rows: Vec<Row>,
    },
    Diagnostics {
        diagnostics: Vec<Diagnostic>,
    },
    SelectedRows {
        rows: Vec<usize>,
    },
    /// `stale` indexes into the submitted batch, so an agent learns which drafts to re-read rather
    /// than only how many were dropped.
    Staged {
        accepted: usize,
        stale: Vec<usize>,
    },
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
    TooManyFindings,
    EmptyFindingMessage,
    FindingMessageTooLong,
}

#[cfg(test)]
mod tests {
    use super::{
        Finding, MAX_FINDINGS, MAX_MESSAGE_LEN, MAX_QUERY_LEN, MAX_ROWS_PER_REQUEST, Request,
        RequestError, ResultSet, Severity, TableRevision,
    };

    fn finding(message: &str) -> Finding {
        Finding {
            row: 0,
            column: "Title".into(),
            severity: Severity::Warning,
            message: message.into(),
            expected: "Harvest".into(),
            replacement: Some("Harvest, 1962".into()),
        }
    }

    fn staged(findings: Vec<Finding>) -> Result<(), RequestError> {
        Request::StageFindings {
            revision: TableRevision(1),
            findings,
        }
        .validate()
    }

    /// A batch of drafts is bounded before any of it reaches the Problems panel: an agent must not
    /// be able to bury the archivist's own findings under thousands of its own, or push an essay
    /// into a one-line panel row.
    #[test]
    fn staged_findings_are_bounded_and_must_say_something() {
        assert_eq!(staged(vec![finding("date is a guess")]), Ok(()));
        assert_eq!(staged(vec![]), Ok(()), "an empty batch retracts");
        assert_eq!(
            staged(vec![finding("x"); MAX_FINDINGS + 1]),
            Err(RequestError::TooManyFindings)
        );
        assert_eq!(
            staged(vec![finding("  ")]),
            Err(RequestError::EmptyFindingMessage)
        );
        assert_eq!(
            staged(vec![finding(&"x".repeat(MAX_MESSAGE_LEN + 1))]),
            Err(RequestError::FindingMessageTooLong)
        );
    }

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
