//! Shared project reader and export formats for the app and browser.

use rusqlite::Connection;

pub mod columns;
pub mod description;
pub mod export;
pub mod filenames;
pub mod notes;
pub mod photos;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use columns::ColumnType;
pub use export::{
    ArchiveFile, CSL_FIELDS, CslMapping, CsvOptions, ExportComponent, csl_items, csv_bytes,
    derive_csl_mapping, jsonld_hierarchy_value, project_structure_columns, xlsx_bytes, zip_to,
};
pub use notes::{ProjectNote, SheetNote, read_project_notes, sheet_note_request_body, sheet_notes};
pub use photos::PhotoIndex;

pub const QRATE_APPLICATION_ID: i32 = 1097887558;
pub const QRATE_SCHEMA_VERSION: i32 = 4;
pub type LoadedDataset = (Vec<String>, Vec<i64>, Vec<Vec<String>>);
/// Reads `dataset_main` (headers from the table's own columns, then all rows).
/// A project without one (blank) yields empty vecs.
pub fn read_dataset(conn: &Connection) -> rusqlite::Result<LoadedDataset> {
    if !table_exists(conn, "dataset_main")? {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }

    let mut info =
        conn.prepare("SELECT name FROM pragma_table_info('dataset_main') ORDER BY cid")?;
    let headers: Vec<String> = info
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|name| match name {
            Ok(name) if name != "_row_id" && name != "_row_order" => Some(Ok(name)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<rusqlite::Result<_>>()?;
    drop(info);
    let projection = std::iter::once(quote_identifier("_row_id"))
        .chain(headers.iter().map(|header| quote_identifier(header)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT {projection} FROM dataset_main ORDER BY _row_order"
    ))?;
    let n = headers.len();
    let mut row_ids = Vec::new();
    let mut rows = Vec::new();
    let mut query = stmt.query([])?;
    while let Some(row) = query.next()? {
        row_ids.push(row.get(0)?);
        let mut cells = Vec::with_capacity(n);
        for i in 0..n {
            // Cells are written as TEXT, but be tolerant of NULLs.
            cells.push(row.get::<_, Option<String>>(i + 1)?.unwrap_or_default());
        }
        rows.push(cells);
    }
    Ok((headers, row_ids, rows))
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    File,
    Directory,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "directory" => Some(Self::Directory),
            _ => None,
        }
    }
}

/// Private arrangement metadata for one archival component. None of these values becomes a
/// visible dataset column unless the user explicitly maps it during export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowStructure {
    pub row_id: i64,
    pub parent_id: Option<i64>,
    pub level_key: String,
    pub sibling_order: i64,
    pub source_path: Option<String>,
    pub source_kind: Option<SourceKind>,
}

/// Reads `__row_structure` in sibling order. A project without one (flat, or older than v4)
/// yields an empty list: every row is an ungrouped root.
pub fn read_row_structure(conn: &Connection) -> rusqlite::Result<Vec<RowStructure>> {
    if !table_exists(conn, "__row_structure")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT row_id, parent_id, level_key, sibling_order, source_path, source_kind
         FROM __row_structure ORDER BY parent_id, sibling_order, row_id",
    )?;
    stmt.query_map([], |row| {
        let source_kind: Option<String> = row.get(5)?;
        Ok(RowStructure {
            row_id: row.get(0)?,
            parent_id: row.get(1)?,
            level_key: row.get(2)?,
            sibling_order: row.get(3)?,
            source_path: row.get(4)?,
            source_kind: source_kind.as_deref().and_then(SourceKind::parse),
        })
    })?
    .collect()
}
