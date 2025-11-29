use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Represents a column definition in the grid
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnDef {
    pub id: String,
    pub name: String,
    pub col_type: String,
    pub width: i32,
    pub hidden: bool,
}

/// Initialize a new .qrate database file with the required schema
pub fn init_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;

    // Enable WAL mode for better concurrency
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = -64000;
        PRAGMA temp_store = MEMORY;
        ",
    )?;

    // Create metadata table for workspace settings
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    // Create column definitions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _columns (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            type TEXT NOT NULL DEFAULT 'Text',
            width INTEGER NOT NULL DEFAULT 150,
            hidden INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    // Create main data table with dynamic columns
    // We'll start with a simple row_id and add columns dynamically
    conn.execute(
        "CREATE TABLE IF NOT EXISTS data (
            row_id INTEGER PRIMARY KEY AUTOINCREMENT
        )",
        [],
    )?;

    // Create index on row_id for faster lookups
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_data_row_id ON data(row_id)",
        [],
    )?;

    // Store initial metadata
    conn.execute(
        "INSERT OR IGNORE INTO _meta (key, value) VALUES ('version', '1.0')",
        [],
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO _meta (key, value) VALUES ('created_at', datetime('now'))",
        [],
    )?;

    Ok(conn)
}

/// Open an existing .qrate database
pub fn open_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;

    // Enable WAL mode
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = -64000;
        PRAGMA temp_store = MEMORY;
        ",
    )?;

    Ok(conn)
}

/// Get column definitions from the database
pub fn get_columns(conn: &Connection) -> Result<Vec<ColumnDef>> {
    let mut stmt =
        conn.prepare("SELECT id, name, type, width, hidden FROM _columns ORDER BY ROWID")?;

    let columns = stmt
        .query_map([], |row| {
            Ok(ColumnDef {
                id: row.get(0)?,
                name: row.get(1)?,
                col_type: row.get(2)?,
                width: row.get(3)?,
                hidden: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    Ok(columns)
}

/// Add a new column to the database
pub fn add_column(conn: &Connection, column: &ColumnDef, default_value: &str) -> Result<()> {
    // Add to _columns table
    conn.execute(
        "INSERT INTO _columns (id, name, type, width, hidden) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            &column.id,
            &column.name,
            &column.col_type,
            column.width,
            if column.hidden { 1 } else { 0 }
        ],
    )?;

    // Add actual column to data table (TEXT affinity for CSV compatibility)
    let alter_sql = format!(
        "ALTER TABLE data ADD COLUMN [{}] TEXT DEFAULT '{}'",
        column.id, default_value
    );
    conn.execute(&alter_sql, [])?;

    Ok(())
}

/// Update column metadata (width, hidden state, etc.)
pub fn update_column(conn: &Connection, column: &ColumnDef) -> Result<()> {
    conn.execute(
        "UPDATE _columns SET name = ?1, type = ?2, width = ?3, hidden = ?4 WHERE id = ?5",
        params![
            &column.name,
            &column.col_type,
            column.width,
            if column.hidden { 1 } else { 0 },
            &column.id
        ],
    )?;

    Ok(())
}

/// Get a page of data rows
pub fn get_rows(conn: &Connection, limit: u32, offset: u32) -> Result<Vec<serde_json::Value>> {
    // Get column definitions to know what fields to select
    let columns = get_columns(conn)?;

    if columns.is_empty() {
        return Ok(vec![]);
    }

    // Build dynamic SELECT query
    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let select_cols = col_names.join(", ");

    let query = format!(
        "SELECT row_id, {} FROM data ORDER BY row_id LIMIT {} OFFSET {}",
        select_cols, limit, offset
    );

    let mut stmt = conn.prepare(&query)?;

    let rows = stmt
        .query_map([], |row| {
            let mut map = serde_json::Map::new();

            // Add row_id
            let row_id: i64 = row.get(0)?;
            map.insert("row_id".to_string(), serde_json::json!(row_id));

            // Add all column values
            for (idx, col) in columns.iter().enumerate() {
                let value: Option<String> = row.get(idx + 1)?;
                map.insert(
                    col.id.clone(),
                    serde_json::Value::String(value.unwrap_or_default()),
                );
            }

            Ok(serde_json::Value::Object(map))
        })?
        .collect::<Result<Vec<_>>>()?;

    Ok(rows)
}

/// Get total row count
pub fn get_row_count(conn: &Connection) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM data")?;
    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count)
}

/// Update a single cell value
pub fn update_cell(conn: &Connection, row_id: i64, column_id: &str, value: &str) -> Result<()> {
    let query = format!("UPDATE data SET [{}] = ?1 WHERE row_id = ?2", column_id);
    conn.execute(&query, params![value, row_id])?;
    Ok(())
}

/// Insert a new row with initial values
pub fn insert_row(
    conn: &Connection,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<i64> {
    let columns = get_columns(conn)?;

    if columns.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{}", i)).collect();

    let insert_sql = format!(
        "INSERT INTO data ({}) VALUES ({})",
        col_names.join(", "),
        placeholders.join(", ")
    );

    // Store string values to ensure proper lifetimes
    let str_values: Vec<String> = columns
        .iter()
        .map(|col| {
            values
                .get(&col.id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();

    let params_vec: Vec<&dyn rusqlite::ToSql> = str_values
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();

    conn.execute(&insert_sql, rusqlite::params_from_iter(params_vec.iter()))?;

    Ok(conn.last_insert_rowid())
}

/// Delete a row by row_id
pub fn delete_row(conn: &Connection, row_id: i64) -> Result<()> {
    conn.execute("DELETE FROM data WHERE row_id = ?1", params![row_id])?;
    Ok(())
}

/// Import CSV data into the database
pub fn import_csv_data(
    conn: &Connection,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
) -> Result<()> {
    // Clear existing data
    conn.execute("DELETE FROM data", [])?;
    conn.execute("DELETE FROM _columns", [])?;

    // Add columns based on CSV headers
    for (idx, header) in headers.iter().enumerate() {
        let col_id = format!("col_{}", idx);
        let column = ColumnDef {
            id: col_id,
            name: header.clone(),
            col_type: "Text".to_string(),
            width: 150,
            hidden: false,
        };
        add_column(conn, &column, "")?;
    }

    // Insert rows in a transaction for better performance
    let tx = conn.unchecked_transaction()?;

    let columns = get_columns(&tx)?;
    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{}", i)).collect();

    let insert_sql = format!(
        "INSERT INTO data ({}) VALUES ({})",
        col_names.join(", "),
        placeholders.join(", ")
    );

    let mut stmt = tx.prepare(&insert_sql)?;

    for row_values in rows {
        let params: Vec<&dyn rusqlite::ToSql> = row_values
            .iter()
            .map(|v| v as &dyn rusqlite::ToSql)
            .collect();
        stmt.execute(rusqlite::params_from_iter(params.iter()))?;
    }

    drop(stmt);
    tx.commit()?;

    Ok(())
}
