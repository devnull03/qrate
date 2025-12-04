use rusqlite::{params, Connection, Result};
use serde_json;
use std::path::Path;

use crate::layout::types::WindowLayout;

const SCHEMA_VERSION: u32 = 1;

/// Initialize the layout database schema
pub fn init_layout_database(conn: &Connection) -> Result<()> {
    // Enable WAL mode for better concurrency
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = -64000;
        PRAGMA temp_store = MEMORY;
        ",
    )?;

    // Create workspace_layouts table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS workspace_layouts (
            window_id TEXT PRIMARY KEY,
            workspace_path TEXT,
            layout_json TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )",
        [],
    )?;

    // Create index on workspace_path for faster lookups
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_layouts_workspace_path ON workspace_layouts(workspace_path)",
        [],
    )?;

    // Create index on updated_at for cleanup queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_layouts_updated_at ON workspace_layouts(updated_at)",
        [],
    )?;

    Ok(())
}

/// Get the layout database path in the app data directory
pub fn get_layout_db_path(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    // Ensure directory exists
    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {}", e))?;

    Ok(app_data_dir.join("layouts.db"))
}

/// Open or create the layout database connection
pub fn open_layout_database(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    init_layout_database(&conn)?;
    Ok(conn)
}

/// Save a window layout to the database
pub fn save_layout(conn: &Connection, layout: &WindowLayout) -> Result<()> {
    let layout_json = serde_json::to_string(layout).map_err(|e| {
        rusqlite::Error::InvalidColumnType(
            0,
            format!("JSON serialization failed: {}", e),
            rusqlite::types::Type::Text,
        )
    })?;

    conn.execute(
        "INSERT OR REPLACE INTO workspace_layouts 
         (window_id, workspace_path, layout_json, version, updated_at) 
         VALUES (?1, ?2, ?3, ?4, strftime('%s', 'now'))",
        params![
            layout.window_id,
            layout.workspace_path,
            layout_json,
            SCHEMA_VERSION
        ],
    )?;

    Ok(())
}

/// Load a window layout from the database
pub fn load_layout(conn: &Connection, window_id: &str) -> Result<Option<WindowLayout>> {
    let mut stmt =
        conn.prepare("SELECT layout_json FROM workspace_layouts WHERE window_id = ?1")?;

    let layout_result = stmt.query_row([window_id], |row| {
        let json_str: String = row.get(0)?;
        Ok(json_str)
    });

    match layout_result {
        Ok(json_str) => {
            let layout: WindowLayout = serde_json::from_str(&json_str).map_err(|e| {
                rusqlite::Error::InvalidColumnType(
                    0,
                    format!("JSON deserialization failed: {}", e),
                    rusqlite::types::Type::Text,
                )
            })?;
            Ok(Some(layout))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Delete a window layout from the database
pub fn delete_layout(conn: &Connection, window_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM workspace_layouts WHERE window_id = ?1",
        [window_id],
    )?;
    Ok(())
}

/// Get all layouts for a specific workspace path
pub fn get_layouts_by_workspace(
    conn: &Connection,
    workspace_path: &str,
) -> Result<Vec<WindowLayout>> {
    let mut stmt =
        conn.prepare("SELECT layout_json FROM workspace_layouts WHERE workspace_path = ?1")?;

    let layouts = stmt
        .query_map([workspace_path], |row| {
            let json_str: String = row.get(0)?;
            Ok(json_str)
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut result = Vec::new();
    for json_str in layouts {
        match serde_json::from_str::<WindowLayout>(&json_str) {
            Ok(layout) => result.push(layout),
            Err(e) => {
                eprintln!("Failed to deserialize layout: {}", e);
                // Continue with other layouts
            }
        }
    }

    Ok(result)
}

/// Get all window IDs in the database
pub fn get_all_window_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT window_id FROM workspace_layouts")?;
    let window_ids = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(window_ids)
}

/// Clean up old layouts (older than specified days)
pub fn cleanup_old_layouts(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - (days * 24 * 60 * 60);
    let deleted = conn.execute(
        "DELETE FROM workspace_layouts WHERE updated_at < ?1",
        [cutoff],
    )?;
    Ok(deleted)
}

// Tests will be added later with proper test dependencies
