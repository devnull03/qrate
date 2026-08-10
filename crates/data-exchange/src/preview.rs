//! The shape every importer produces: headers, rows, and whatever notes the source carried.
//!
//! A local CSV (read by `project_wizard::data`) and a fetched Google Sheet both land here, so the
//! wizard's folder-matching and column-config steps only ever see one type.

use crate::sheet::{SheetData, a1_to_index};

/// A note carried in from the sheet, resolved against this preview's own indices: `row` indexes
/// [`SpreadsheetPreview::rows`] and `column` is the header text, so nothing downstream has to
/// know about spreadsheet coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewNote {
    pub row: usize,
    pub column: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct SpreadsheetPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Cell notes, when the source carried any. CSV has no way to express them.
    pub notes: Vec<PreviewNote>,
}

impl From<SheetData> for SpreadsheetPreview {
    fn from(sheet: SheetData) -> Self {
        let (top, left) = sheet.origin;
        let notes = sheet
            .notes
            .iter()
            .filter_map(|n| {
                // Note refs are absolute sheet coordinates; the rows start at the used range's
                // origin, and the first of those is the header.
                let (r, c) = a1_to_index(&n.cell)?;
                let row = r.checked_sub(top)?.checked_sub(1)? as usize;
                let column = sheet.headers.get(c.checked_sub(left)? as usize)?.clone();
                // An orphan outside the imported grid can't be shown or jumped to.
                (row < sheet.rows.len()).then(|| PreviewNote {
                    row,
                    column,
                    text: n.text.clone(),
                })
            })
            .collect();
        Self {
            headers: sheet.headers,
            rows: sheet.rows,
            notes,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SpreadsheetError {
    NotCsv,
    Empty,
    NoHeaderRow,
    Io(String),
}

impl SpreadsheetError {
    pub fn message(&self) -> String {
        match self {
            SpreadsheetError::NotCsv => "That doesn't look like a CSV file".into(),
            SpreadsheetError::Empty => "This spreadsheet has no rows yet".into(),
            SpreadsheetError::NoHeaderRow => {
                "We couldn't find a header row — the first row looks like data".into()
            }
            SpreadsheetError::Io(e) => format!("We couldn't open that file — {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PreviewNote, SpreadsheetPreview};
    use crate::sheet::{CellNote, SheetData};

    fn sheet(origin: (u32, u32), notes: &[(&str, &str)]) -> SheetData {
        SheetData {
            headers: vec!["Digital ID".into(), "Title".into()],
            rows: vec![
                vec!["1".into(), "First".into()],
                vec!["2".into(), "Second".into()],
            ],
            notes: notes
                .iter()
                .map(|(cell, text)| CellNote {
                    cell: (*cell).into(),
                    text: (*text).into(),
                })
                .collect(),
            origin,
        }
    }

    #[test]
    fn sheet_notes_resolve_against_the_used_range_origin() {
        // Sheet starting at A1: row 1 is the header, so B2 is the first data row's `Title`.
        let p = SpreadsheetPreview::from(sheet((0, 0), &[("B2", "check this")]));
        assert_eq!(
            p.notes,
            vec![PreviewNote {
                row: 0,
                column: "Title".into(),
                text: "check this".into(),
            }]
        );

        // Same grid parked at C5: calamine's rows start there, but note refs stay absolute, so
        // the origin has to come off both axes or every note lands in the wrong cell.
        let p = SpreadsheetPreview::from(sheet((4, 2), &[("D6", "check this")]));
        assert_eq!(
            p.notes,
            vec![PreviewNote {
                row: 0,
                column: "Title".into(),
                text: "check this".into(),
            }]
        );
    }

    #[test]
    fn notes_outside_the_imported_grid_are_dropped() {
        // The header row itself, a row past the last one, a column past the last one, and junk.
        let p = SpreadsheetPreview::from(sheet(
            (0, 0),
            &[
                ("A1", "header"),
                ("A9", "too far down"),
                ("Z2", "too far right"),
                ("nope", "junk"),
            ],
        ));
        assert_eq!(p.notes, Vec::new());
    }
}
