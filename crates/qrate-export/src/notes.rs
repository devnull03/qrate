//! Authored project notes and their spreadsheet coordinates.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{Connection, OptionalExtension as _};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectNote {
    pub dataset: String,
    pub row_id: Option<i64>,
    pub column: Option<String>,
    pub severity: String,
    pub message: String,
    pub created_at: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetNote {
    /// Zero-based sheet row; row zero contains column headers.
    pub row: u32,
    pub col: u16,
    pub text: String,
}

/// Older projects may not have a notes table or provenance columns yet.
pub fn read_project_notes(conn: &Connection) -> rusqlite::Result<Vec<ProjectNote>> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='__notes'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    if !exists {
        return Ok(Vec::new());
    }
    let provenance: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('__notes') WHERE name='created_at'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    let columns = if provenance {
        "created_at, author"
    } else {
        "NULL AS created_at, NULL AS author"
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT dataset, row_ix, column_name, severity, message, {columns} FROM __notes"
    ))?;
    stmt.query_map([], |row| {
        Ok(ProjectNote {
            dataset: row.get(0)?,
            row_id: row.get(1)?,
            column: row.get(2)?,
            severity: row.get(3)?,
            message: row.get(4)?,
            created_at: row.get(5)?,
            author: row.get(6)?,
        })
    })?
    .collect()
}

/// Merge every note that lands on the same spreadsheet cell. Wider notes use the first column or
/// first header cell, with their scope named in the text so they retain their meaning.
pub fn sheet_notes(
    headers: &[String],
    row_ids: &[i64],
    column_notes: &[(String, String)],
    notes: &[ProjectNote],
) -> Vec<SheetNote> {
    if headers.is_empty() {
        return Vec::new();
    }
    let rows: HashMap<i64, usize> = row_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut cells: BTreeMap<(u32, u16), Vec<String>> = BTreeMap::new();
    for (name, text) in column_notes {
        if !text.trim().is_empty()
            && let Some(col) = headers.iter().position(|header| header == name)
        {
            cells.entry((0, col as u16)).or_default().push(text.clone());
        }
    }
    for note in notes {
        if note.message.trim().is_empty() {
            continue;
        }
        let row = if note.dataset == "dataset_main" {
            note.row_id
                .and_then(|id| rows.get(&id).copied())
                .map(|index| index as u32 + 1)
        } else {
            None
        };
        let col = note
            .column
            .as_ref()
            .and_then(|name| headers.iter().position(|header| header == name));
        let destination = (row.unwrap_or(0), col.unwrap_or(0) as u16);
        let mut scope = Vec::new();
        if note.dataset != "dataset_main" {
            scope.push(format!("Dataset: {}", note.dataset));
        }
        if let Some(row_id) = note.row_id.filter(|_| row.is_none()) {
            scope.push(format!("Row ID: {row_id}"));
        } else if note.row_id.is_none() && note.column.is_none() {
            scope.push("Dataset note".into());
        } else if note.row_id.is_some() && note.column.is_none() {
            scope.push("Row note".into());
        }
        if let Some(column) = note.column.as_ref().filter(|_| col.is_none()) {
            scope.push(format!("Column: {column}"));
        }
        if note.severity != "note" {
            scope.push(format!("{} note", note.severity));
        }
        if let Some(author) = note.author.as_ref().filter(|s| !s.trim().is_empty()) {
            scope.push(author.clone());
        }
        if let Some(date) = note.created_at.as_ref().filter(|s| !s.trim().is_empty()) {
            scope.push(date.clone());
        }
        let text = if scope.is_empty() {
            note.message.clone()
        } else {
            format!("{}\n{}", scope.join(" · "), note.message)
        };
        cells.entry(destination).or_default().push(text);
    }
    cells
        .into_iter()
        .map(|((row, col), parts)| SheetNote {
            row,
            col,
            text: parts.join("\n\n"),
        })
        .collect()
}

/// The second Sheets call after values.update. Its field mask touches only `note`, leaving every
/// cell value intact. Existing notes in cells without a project note are left alone.
pub fn sheet_note_request_body(sheet_id: i64, notes: &[SheetNote]) -> Value {
    let mut requests = Vec::new();
    for note in notes {
        requests.push(json!({
            "updateCells": {
                "start": { "sheetId": sheet_id, "rowIndex": note.row, "columnIndex": note.col },
                "rows": [{ "values": [{ "note": note.text }] }],
                "fields": "note"
            }
        }));
    }
    json!({ "requests": requests })
}

#[cfg(test)]
mod tests {
    use super::{ProjectNote, sheet_note_request_body, sheet_notes};

    #[test]
    fn maps_cell_column_row_and_dataset_notes() {
        let headers = vec!["Title".into(), "Date".into()];
        let note = |row_id: Option<i64>, column: Option<&str>, message: &str| ProjectNote {
            dataset: "dataset_main".into(),
            row_id,
            column: column.map(str::to_string),
            severity: "note".into(),
            message: message.into(),
            author: None,
            created_at: None,
        };
        let placed = sheet_notes(
            &headers,
            &[8, 3],
            &[("Date".into(), "ISO or EDTF".into())],
            &[
                note(Some(3), Some("Date"), "circa"),
                note(Some(8), None, "fragile"),
                note(None, None, "private catalogue"),
            ],
        );
        assert_eq!(placed.len(), 4);
        assert_eq!((placed[0].row, placed[0].col), (0, 0));
        assert_eq!(
            (placed[1].row, placed[1].col, placed[1].text.as_str()),
            (0, 1, "ISO or EDTF")
        );
        assert_eq!((placed[2].row, placed[2].col), (1, 0));
        assert_eq!(
            (placed[3].row, placed[3].col, placed[3].text.as_str()),
            (2, 1, "circa")
        );
        let body = sheet_note_request_body(42, &placed);
        assert_eq!(body["requests"][0]["updateCells"]["fields"], "note");
        assert_eq!(body["requests"][3]["updateCells"]["start"]["rowIndex"], 2);
    }
}
