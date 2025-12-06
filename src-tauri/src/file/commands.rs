use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::database::{self, ColumnDef, ProjectConfig, ProjectMode};

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
            error: Some("No base folder configured".into()),
        });
    }

    let base_path = Path::new(&base_folder);
    let file_path_obj = Path::new(&file_path);

    let canonical_base = base_path
        .canonicalize()
        .or_else(|_| std::path::absolute(base_path))
        .map_err(|e| format!("Invalid base folder path: {}", e))?;

    let canonical_file = file_path_obj
        .canonicalize()
        .or_else(|_| std::path::absolute(file_path_obj))
        .map_err(|e| format!("Invalid file path: {}", e))?;

    if !canonical_file.starts_with(&canonical_base) {
        return Ok(FilePathValidationResponse {
            valid: false,
            resolved_path: canonical_file.to_string_lossy().into(),
            error: Some("File is outside the trusted folder".into()),
        });
    }

    Ok(FilePathValidationResponse {
        valid: true,
        resolved_path: canonical_file.to_string_lossy().into(),
        error: None,
    })
}

#[tauri::command]
pub fn get_current_state(state: State<AppState>) -> Result<CurrentStateResponse, String> {
    let Some(file_state) = state.get_current_file() else {
        return Ok(CurrentStateResponse {
            is_file_open: false,
            path: None,
            columns: vec![],
            total_rows: 0,
            rows: vec![],
            offset: 0,
            limit: 100,
        });
    };

    let conn_arc = state
        .get_connection(&file_state.path)
        .ok_or("Project connection not found")?;
    let conn = conn_arc.lock().unwrap();

    Ok(CurrentStateResponse {
        is_file_open: true,
        path: Some(file_state.path.clone()),
        columns: database::get_columns(&conn).map_err(|e| e.to_string())?,
        total_rows: database::get_row_count(&conn).map_err(|e| e.to_string())?,
        rows: database::get_rows(&conn, file_state.limit, file_state.offset)
            .map_err(|e| e.to_string())?,
        offset: file_state.offset,
        limit: file_state.limit,
    })
}

#[tauri::command]
pub fn create_qrate_project(
    state: State<AppState>,
    path: String,
    name: Option<String>,
    mode: Option<String>,
    images_root: Option<String>,
) -> Result<FileOpenResponse, String> {
    let project_path = PathBuf::from(&path);
    std::fs::create_dir_all(&project_path).map_err(|e| e.to_string())?;

    let project_name = name.unwrap_or_else(|| {
        project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled Project")
            .to_string()
    });

    let project_mode = match mode.as_deref() {
        Some("copy") => ProjectMode::Copy,
        _ => ProjectMode::Reference,
    };

    let config = ProjectConfig::new(project_name, project_mode, images_root);
    let conn = database::init_database_with_config(&project_path, Some(config))
        .map_err(|e| e.to_string())?;
    let columns = database::get_columns(&conn).map_err(|e| e.to_string())?;
    let total_rows = database::get_row_count(&conn).map_err(|e| e.to_string())?;

    state.add_connection(path.clone(), conn);
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

#[tauri::command]
pub fn open_qrate_project(
    state: State<AppState>,
    path: String,
) -> Result<FileOpenResponse, String> {
    let project_path = PathBuf::from(&path);

    if !database::is_qrate_project(&project_path) {
        return Err(format!("Not a qrate project: {}", path));
    }

    let conn = database::open_database(&project_path).map_err(|e| e.to_string())?;
    let columns = database::get_columns(&conn).map_err(|e| e.to_string())?;
    let total_rows = database::get_row_count(&conn).map_err(|e| e.to_string())?;

    state.add_connection(path.clone(), conn);
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

#[tauri::command]
pub fn close_qrate_project(state: State<AppState>, path: String) -> Result<(), String> {
    if let Some(conn_arc) = state.remove_connection(&path) {
        if let Ok(conn) = conn_arc.lock() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
    if state
        .get_current_file()
        .map(|c| c.path == path)
        .unwrap_or(false)
    {
        state.clear_current_file();
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
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();

    let rows = database::get_rows(&conn, limit, offset).map_err(|e| e.to_string())?;
    let total = database::get_row_count(&conn).map_err(|e| e.to_string())?;

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
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();
    database::update_cell(&conn, row_id, &column_id, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_column(state: State<AppState>, path: String, column: ColumnDef) -> Result<(), String> {
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();
    database::add_column(&conn, &column, "").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_column(
    state: State<AppState>,
    path: String,
    column: ColumnDef,
) -> Result<(), String> {
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();
    database::update_column(&conn, &column).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn insert_row(
    state: State<AppState>,
    path: String,
    values: serde_json::Map<String, serde_json::Value>,
) -> Result<i64, String> {
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();
    database::insert_row(&conn, &values).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_row(state: State<AppState>, path: String, row_id: i64) -> Result<(), String> {
    let conn_arc = state.get_connection(&path).ok_or("Project not open")?;
    let conn = conn_arc.lock().unwrap();
    database::delete_row(&conn, row_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_csv_to_qrate(
    state: State<AppState>,
    qrate_path: String,
    csv_path: String,
) -> Result<FileOpenResponse, String> {
    let mut reader = csv::Reader::from_path(&csv_path).map_err(|e| e.to_string())?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        rows.push(
            record
                .map_err(|e| e.to_string())?
                .iter()
                .map(|f| f.to_string())
                .collect(),
        );
    }

    let project_path = PathBuf::from(&qrate_path);
    let is_new = !database::is_qrate_project(&project_path);

    if is_new {
        std::fs::create_dir_all(&project_path).map_err(|e| e.to_string())?;
    }

    // Copy the CSV file into the project folder
    let csv_source_path = PathBuf::from(&csv_path);
    let relative_csv_path = database::copy_csv_to_project(&project_path, &csv_source_path)
        .map_err(|e| format!("Failed to copy CSV to project: {}", e))?;

    let conn = if is_new {
        database::init_database(&project_path)
    } else {
        database::open_database(&project_path)
    }
    .map_err(|e| e.to_string())?;

    database::import_csv_data(&conn, headers, rows).map_err(|e| e.to_string())?;

    // Update project config with source CSV path
    if let Ok(mut config) = database::load_project_config(&project_path) {
        config.paths.source_csv = Some(relative_csv_path);
        let _ = database::save_project_config(&project_path, &config);
    }

    let columns = database::get_columns(&conn).map_err(|e| e.to_string())?;
    let total_rows = database::get_row_count(&conn).map_err(|e| e.to_string())?;

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
    let mut reader = csv::Reader::from_path(&csv_path).map_err(|e| e.to_string())?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let first_row = reader
        .records()
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .map(|r| r.iter().map(|f| f.to_string()).collect());

    Ok(CsvPreviewResponse { headers, first_row })
}
