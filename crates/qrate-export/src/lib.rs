//! Shared project reader and export formats for the app and browser.

use rusqlite::Connection;

pub mod columns;
pub mod export;
pub mod filenames;
pub mod photos;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use columns::ColumnType;
pub use export::{
    ArchiveFile, CSL_FIELDS, CslMapping, ExportComponent, csl_items, csv_bytes, derive_csl_mapping,
    jsonld_hierarchy_value, project_structure_columns, xlsx_bytes, zip_to,
};
pub use photos::PhotoIndex;

pub const QRATE_APPLICATION_ID: i32 = 1097887558;
pub const QRATE_SCHEMA_VERSION: i32 = 4;
pub type LoadedDataset = (Vec<String>, Vec<i64>, Vec<Vec<String>>);
/// Reads `dataset_main` (headers from the table's own columns, then all rows).
/// A project without one (blank) yields empty vecs.
pub fn read_dataset(conn: &Connection) -> rusqlite::Result<LoadedDataset> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'dataset_main'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
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
