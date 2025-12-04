use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::database::{self, ColumnDef};

use super::types::{
    CsvPreviewResponse, CurrentStateResponse, DataResponse, FileOpenResponse,
    FilePathValidationResponse,
};

#[tauri::command]
pub fn validate_file_path(
    file_path: String,
    base_folder: String,
) -> Result<FilePathValidationResponse, String> {
    use std::path::Path;

    if base_folder.is_empty() {
        return Ok(FilePathValidationResponse {
            valid: false,
            resolved_path: String::new(),
            error: Some("No base folder configured".to_string()),
        });
    }

    let file_path_obj = Path::new(&file_path);
    let base_path = Path::new(&base_folder);

    let canonical_base = match base_path.canonicalize() {
        Ok(p) => p,
        Err(_) => match std::path::absolute(base_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(FilePathValidationResponse {
                    valid: false,
                    resolved_path: String::new(),
                    error: Some(format!("Invalid base folder path: {}", e)),
                })
            }
        },
    };

    let canonical_file = match file_path_obj.canonicalize() {
        Ok(p) => p,
        Err(_) => match std::path::absolute(file_path_obj) {
            Ok(p) => p,
            Err(e) => {
                return Ok(FilePathValidationResponse {
                    valid: false,
                    resolved_path: String::new(),
                    error: Some(format!("Invalid file path: {}", e)),
                })
            }
        },
    };

    if !canonical_file.starts_with(&canonical_base) {
        return Ok(FilePathValidationResponse {
            valid: false,
            resolved_path: canonical_file.to_string_lossy().to_string(),
            error: Some("File is outside the trusted folder".to_string()),
        });
    }

    Ok(FilePathValidationResponse {
        valid: true,
        resolved_path: canonical_file.to_string_lossy().to_string(),
        error: None,
    })
}

#[tauri::command]
pub fn get_current_state(state: State<AppState>) -> Result<CurrentStateResponse, String> {
    let current_file = state.get_current_file();

    match current_file {
        Some(file_state) => {
            let conn_arc = state
                .get_connection(&file_state.path)
                .ok_or_else(|| "File connection not found".to_string())?;

            let conn = conn_arc.lock().unwrap();

            let columns = database::get_columns(&conn)
                .map_err(|e| format!("Failed to get columns: {}", e))?;

            let total_rows = database::get_row_count(&conn)
                .map_err(|e| format!("Failed to get row count: {}", e))?;

            let rows = database::get_rows(&conn, file_state.limit, file_state.offset)
                .map_err(|e| format!("Failed to get rows: {}", e))?;

            Ok(CurrentStateResponse {
                is_file_open: true,
                path: Some(file_state.path),
                columns,
                total_rows,
                rows,
                offset: file_state.offset,
                limit: file_state.limit,
            })
        }
        None => Ok(CurrentStateResponse {
            is_file_open: false,
            path: None,
            columns: vec![],
            total_rows: 0,
            rows: vec![],
            offset: 0,
            limit: 100,
        }),
    }
}

#[tauri::command]
pub fn create_qrate_file(state: State<AppState>, path: String) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    let conn = database::init_database(&path_buf)
        .map_err(|e| format!("Failed to create database: {}", e))?;

    std::fs::write(&path_buf, "").map_err(|e| format!("Failed to create marker file: {}", e))?;

    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    state.add_connection(path.clone(), conn);
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

#[tauri::command]
pub fn open_qrate_file(state: State<AppState>, path: String) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    let conn = database::open_database(&path_buf)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    state.add_connection(path.clone(), conn);
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

#[tauri::command]
pub fn close_qrate_file(state: State<AppState>, path: String) -> Result<(), String> {
    if let Some(conn_arc) = state.remove_connection(&path) {
        if let Ok(conn) = conn_arc.lock() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }

    if let Some(current) = state.get_current_file() {
        if current.path == path {
            state.clear_current_file();
        }
    }

    Ok(())
}

#[tauri::command]
pub fn get_rows(
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

    let total =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    state.update_viewport(offset, limit);

    Ok(DataResponse { rows, total })
}

#[tauri::command]
pub fn update_cell(
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

#[tauri::command]
pub fn add_column(state: State<AppState>, path: String, column: ColumnDef) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::add_column(&conn, &column, "").map_err(|e| format!("Failed to add column: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn update_column(
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

#[tauri::command]
pub fn insert_row(
    state: State<AppState>,
    path: String,
    values: serde_json::Map<String, serde_json::Value>,
) -> Result<i64, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let row_id =
        database::insert_row(&conn, &values).map_err(|e| format!("Failed to insert row: {}", e))?;

    Ok(row_id)
}

#[tauri::command]
pub fn delete_row(state: State<AppState>, path: String, row_id: i64) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::delete_row(&conn, row_id).map_err(|e| format!("Failed to delete row: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn import_csv_to_qrate(
    state: State<AppState>,
    qrate_path: String,
    csv_path: String,
) -> Result<FileOpenResponse, String> {
    let mut reader =
        csv::Reader::from_path(&csv_path).map_err(|e| format!("Failed to open CSV file: {}", e))?;

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

    let path_buf = PathBuf::from(&qrate_path);
    let is_new = !path_buf.exists();
    let conn = if is_new {
        database::init_database(&path_buf)
    } else {
        database::open_database(&path_buf)
    }
    .map_err(|e| format!("Failed to open/create database: {}", e))?;

    if is_new {
        std::fs::write(&path_buf, "")
            .map_err(|e| format!("Failed to create marker file: {}", e))?;
    }

    database::import_csv_data(&conn, headers, rows)
        .map_err(|e| format!("Failed to import CSV data: {}", e))?;

    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    state.add_connection(qrate_path.clone(), conn);
    state.set_current_file(qrate_path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path: qrate_path,
        columns,
        total_rows,
    })
}

#[tauri::command]
pub fn preview_csv(csv_path: String) -> Result<CsvPreviewResponse, String> {
    let mut reader =
        csv::Reader::from_path(&csv_path).map_err(|e| format!("Failed to open CSV file: {}", e))?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Failed to read CSV headers: {}", e))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let first_row = if let Some(record) = reader.records().next() {
        let record = record.map_err(|e| format!("Failed to read CSV record: {}", e))?;
        Some(record.iter().map(|f| f.to_string()).collect())
    } else {
        None
    };

    Ok(CsvPreviewResponse { headers, first_row })
}
