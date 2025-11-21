use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

mod app_state;
mod database;

use app_state::AppState;
use database::ColumnDef;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Response structure for file operations
#[derive(Debug, Serialize, Deserialize)]
struct FileOpenResponse {
    path: String,
    columns: Vec<ColumnDef>,
    total_rows: i64,
}

/// Response structure for data fetch operations
#[derive(Debug, Serialize, Deserialize)]
struct DataResponse {
    rows: Vec<serde_json::Value>,
    total: i64,
}

/// Create a new .qrate file
#[tauri::command]
fn create_qrate_file(
    state: State<AppState>,
    path: String,
) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    // Initialize new database
    let conn = database::init_database(&path_buf)
        .map_err(|e| format!("Failed to create database: {}", e))?;

    // Get initial state
    let columns = database::get_columns(&conn)
        .map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows = database::get_row_count(&conn)
        .map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(path.clone(), conn);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

/// Open an existing .qrate file
#[tauri::command]
fn open_qrate_file(
    state: State<AppState>,
    path: String,
) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    // Open existing database
    let conn = database::open_database(&path_buf)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Get current state
    let columns = database::get_columns(&conn)
        .map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows = database::get_row_count(&conn)
        .map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(path.clone(), conn);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

/// Close a .qrate file
#[tauri::command]
fn close_qrate_file(
    state: State<AppState>,
    path: String,
) -> Result<(), String> {
    state.remove_connection(&path);
    Ok(())
}

/// Get a page of rows from the database
#[tauri::command]
fn get_rows(
    state: State<AppState>,
    path: String,
    limit: u32,
    offset: u32,
) -> Result<DataResponse, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let rows = database::get_rows(&conn, limit, offset)
        .map_err(|e| format!("Failed to get rows: {}", e))?;

    let total = database::get_row_count(&conn)
        .map_err(|e| format!("Failed to get row count: {}", e))?;

    Ok(DataResponse { rows, total })
}

/// Update a single cell value
#[tauri::command]
fn update_cell(
    state: State<AppState>,
    path: String,
    row_id: i64,
    column_id: String,
    value: String,
) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::update_cell(&conn, row_id, &column_id, &value)
        .map_err(|e| format!("Failed to update cell: {}", e))?;

    Ok(())
}

/// Add a new column
#[tauri::command]
fn add_column(
    state: State<AppState>,
    path: String,
    column: ColumnDef,
) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::add_column(&conn, &column, "")
        .map_err(|e| format!("Failed to add column: {}", e))?;

    Ok(())
}

/// Update column metadata (width, hidden state, etc.)
#[tauri::command]
fn update_column(
    state: State<AppState>,
    path: String,
    column: ColumnDef,
) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::update_column(&conn, &column)
        .map_err(|e| format!("Failed to update column: {}", e))?;

    Ok(())
}

/// Insert a new row
#[tauri::command]
fn insert_row(
    state: State<AppState>,
    path: String,
    values: serde_json::Map<String, serde_json::Value>,
) -> Result<i64, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let row_id = database::insert_row(&conn, &values)
        .map_err(|e| format!("Failed to insert row: {}", e))?;

    Ok(row_id)
}

/// Delete a row
#[tauri::command]
fn delete_row(
    state: State<AppState>,
    path: String,
    row_id: i64,
) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::delete_row(&conn, row_id)
        .map_err(|e| format!("Failed to delete row: {}", e))?;

    Ok(())
}

/// Import CSV data into a .qrate file
#[tauri::command]
fn import_csv_to_qrate(
    state: State<AppState>,
    qrate_path: String,
    csv_path: String,
) -> Result<FileOpenResponse, String> {
    // Read CSV file
    let mut reader = csv::Reader::from_path(&csv_path)
        .map_err(|e| format!("Failed to open CSV file: {}", e))?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Failed to read CSV headers: {}", e))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| format!("Failed to read CSV record: {}", e))?;
        let row: Vec<String> = record.iter().map(|f| f.to_string()).collect();
        rows.push(row);
    }

    // Open or create qrate file
    let path_buf = PathBuf::from(&qrate_path);
    let conn = if path_buf.exists() {
        database::open_database(&path_buf)
    } else {
        database::init_database(&path_buf)
    }
    .map_err(|e| format!("Failed to open/create database: {}", e))?;

    // Import data
    database::import_csv_data(&conn, headers, rows)
        .map_err(|e| format!("Failed to import CSV data: {}", e))?;

    // Get final state
    let columns = database::get_columns(&conn)
        .map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows = database::get_row_count(&conn)
        .map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(qrate_path.clone(), conn);

    Ok(FileOpenResponse {
        path: qrate_path,
        columns,
        total_rows,
    })
}

/// Legacy CSV loader for backward compatibility
#[tauri::command]
fn load_csv(path: String) -> Result<Vec<std::collections::HashMap<String, String>>, String> {
    let mut reader = csv::Reader::from_path(&path)
        .map_err(|e| format!("Failed to open CSV file: {}", e))?;

    let headers = reader
        .headers()
        .map_err(|e| format!("Failed to read CSV headers: {}", e))?
        .clone();

    let mut result = Vec::new();

    for record in reader.records() {
        let record = record.map_err(|e| format!("Failed to read CSV record: {}", e))?;
        let mut row = std::collections::HashMap::new();

        for (header, value) in headers.iter().zip(record.iter()) {
            row.insert(header.to_string(), value.to_string());
        }

        result.push(row);
    }

    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs_pro::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            greet,
            create_qrate_file,
            open_qrate_file,
            close_qrate_file,
            get_rows,
            update_cell,
            add_column,
            update_column,
            insert_row,
            delete_row,
            import_csv_to_qrate,
            load_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
