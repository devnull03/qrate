use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::ImageReader;
use std::io::Cursor;

mod app_state;
mod database;
pub mod layout;
mod layout_state;
pub mod settings;
pub mod window;

use app_state::AppState;
use database::ColumnDef;
use layout::types::{WindowLayout, ChatMode};
use layout::manager::LayoutManager;
use layout::persistence::get_layout_db_path;
use layout_state::LayoutState;
use window::manager::WindowManager;
use window::registry::WindowInfo;

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

/// Response structure for current state (used by main window on init)
#[derive(Debug, Serialize, Deserialize)]
struct CurrentStateResponse {
    is_file_open: bool,
    path: Option<String>,
    columns: Vec<ColumnDef>,
    total_rows: i64,
    rows: Vec<serde_json::Value>,
    offset: u32,
    limit: u32,
}

/// Response structure for file path validation
#[derive(Debug, Serialize, Deserialize)]
struct FilePathValidationResponse {
    valid: bool,
    resolved_path: String,
    error: Option<String>,
}

/// Response structure for image loading
#[derive(Debug, Serialize, Deserialize)]
struct ImageResponse {
    data: String,
    mime_type: String,
}

/// Load an image from disk, optionally resizing it for thumbnails
#[tauri::command]
fn load_image(file_path: String, max_width: u32, max_height: u32) -> Result<ImageResponse, String> {
    use std::path::Path;

    let path = Path::new(&file_path);

    // Check if file exists
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Determine MIME type from extension
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let mime_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => return Err(format!("Unsupported image format: {}", ext)),
    };

    // For SVG, just read and return as-is (no resizing)
    if ext == "svg" {
        let content = std::fs::read(&file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        return Ok(ImageResponse {
            data: BASE64.encode(&content),
            mime_type: mime_type.to_string(),
        });
    }

    // Load and decode the image
    let img = ImageReader::open(&file_path)
        .map_err(|e| format!("Failed to open image: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    // Resize if necessary (maintaining aspect ratio)
    let resized = if img.width() > max_width || img.height() > max_height {
        img.thumbnail(max_width, max_height)
    } else {
        img
    };

    // Encode to the appropriate format
    let mut buffer = Cursor::new(Vec::new());

    let output_format = match ext.as_str() {
        "png" => image::ImageFormat::Png,
        "gif" => image::ImageFormat::Gif,
        "webp" => image::ImageFormat::WebP,
        "bmp" => image::ImageFormat::Bmp,
        _ => image::ImageFormat::Jpeg, // Default to JPEG for compression
    };

    resized.write_to(&mut buffer, output_format)
        .map_err(|e| format!("Failed to encode image: {}", e))?;

    Ok(ImageResponse {
        data: BASE64.encode(buffer.into_inner()),
        mime_type: mime_type.to_string(),
    })
}

/// Validate a file path to ensure it's within a trusted base directory
/// This prevents path traversal attacks and ensures files are opened safely
#[tauri::command]
fn validate_file_path(file_path: String, base_folder: String) -> Result<FilePathValidationResponse, String> {
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

    // Canonicalize the base path (resolve symlinks, .., etc.)
    let canonical_base = match base_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Base path might not exist yet, use the absolute path
            match std::path::absolute(base_path) {
                Ok(p) => p,
                Err(e) => return Ok(FilePathValidationResponse {
                    valid: false,
                    resolved_path: String::new(),
                    error: Some(format!("Invalid base folder path: {}", e)),
                }),
            }
        }
    };

    // Canonicalize the file path
    let canonical_file = match file_path_obj.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // File might not exist yet, use the absolute path
            match std::path::absolute(file_path_obj) {
                Ok(p) => p,
                Err(e) => return Ok(FilePathValidationResponse {
                    valid: false,
                    resolved_path: String::new(),
                    error: Some(format!("Invalid file path: {}", e)),
                }),
            }
        }
    };

    // Check if the file path starts with the base path
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

/// Get the current application state (for main window initialization)
#[tauri::command]
fn get_current_state(state: State<AppState>) -> Result<CurrentStateResponse, String> {
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

/// Create a new .qrate file
#[tauri::command]
fn create_qrate_file(state: State<AppState>, path: String) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    // Initialize new database
    let conn = database::init_database(&path_buf)
        .map_err(|e| format!("Failed to create database: {}", e))?;

    // Get initial state
    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(path.clone(), conn);

    // Set as current file
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

/// Open an existing .qrate file
#[tauri::command]
fn open_qrate_file(state: State<AppState>, path: String) -> Result<FileOpenResponse, String> {
    let path_buf = PathBuf::from(&path);

    // Open existing database
    let conn = database::open_database(&path_buf)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Get current state
    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(path.clone(), conn);

    // Set as current file
    state.set_current_file(path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path,
        columns,
        total_rows,
    })
}

/// Close a .qrate file
#[tauri::command]
fn close_qrate_file(state: State<AppState>, path: String) -> Result<(), String> {
    state.remove_connection(&path);

    // Clear current file if it matches
    if let Some(current) = state.get_current_file() {
        if current.path == path {
            state.clear_current_file();
        }
    }

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

    let total =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    // Update viewport in current file state
    state.update_viewport(offset, limit);

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
fn add_column(state: State<AppState>, path: String, column: ColumnDef) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::add_column(&conn, &column, "").map_err(|e| format!("Failed to add column: {}", e))?;

    Ok(())
}

/// Update column metadata (width, hidden state, etc.)
#[tauri::command]
fn update_column(state: State<AppState>, path: String, column: ColumnDef) -> Result<(), String> {
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

    let row_id =
        database::insert_row(&conn, &values).map_err(|e| format!("Failed to insert row: {}", e))?;

    Ok(row_id)
}

/// Delete a row
#[tauri::command]
fn delete_row(state: State<AppState>, path: String, row_id: i64) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    database::delete_row(&conn, row_id).map_err(|e| format!("Failed to delete row: {}", e))?;

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
    let columns =
        database::get_columns(&conn).map_err(|e| format!("Failed to get columns: {}", e))?;

    let total_rows =
        database::get_row_count(&conn).map_err(|e| format!("Failed to get row count: {}", e))?;

    // Store connection in app state
    state.add_connection(qrate_path.clone(), conn);

    // Set as current file
    state.set_current_file(qrate_path.clone(), 0, 100);

    Ok(FileOpenResponse {
        path: qrate_path,
        columns,
        total_rows,
    })
}

/// Show the main window and hide the projects window
#[tauri::command]
fn show_main_window(app: AppHandle) -> Result<(), String> {
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Hide projects window instead of closing it
    if let Some(projects_window) = app.get_webview_window("projects") {
        let _ = projects_window.hide();
    }

    main_window
        .show()
        .map_err(|e| format!("Failed to show main window: {}", e))?;
    main_window
        .set_focus()
        .map_err(|e| format!("Failed to focus main window: {}", e))?;

    Ok(())
}

/// Show the projects window
#[tauri::command]
fn show_projects_window(app: AppHandle) -> Result<(), String> {
    let projects_window = app
        .get_webview_window("projects")
        .ok_or_else(|| "Projects window not found".to_string())?;

    projects_window
        .show()
        .map_err(|e| format!("Failed to show projects window: {}", e))?;
    projects_window
        .set_focus()
        .map_err(|e| format!("Failed to focus projects window: {}", e))?;

    Ok(())
}

/// Show the settings window
#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<(), String> {
    // Try to get existing settings window or create a new one
    if let Some(settings_window) = app.get_webview_window("settings") {
        settings_window
            .show()
            .map_err(|e| format!("Failed to show settings window: {}", e))?;
        settings_window
            .set_focus()
            .map_err(|e| format!("Failed to focus settings window: {}", e))?;
    } else {
        // Create settings window if it doesn't exist
        let settings_window = tauri::WebviewWindowBuilder::new(
            &app,
            "settings",
            tauri::WebviewUrl::App("/settings".into()),
        )
        .title("qRate - Settings")
        .inner_size(600.0, 700.0)
        .min_inner_size(400.0, 500.0)
        .decorations(false)
        .visible(true)
        .center()
        .build()
        .map_err(|e| format!("Failed to create settings window: {}", e))?;

        settings_window
            .show()
            .map_err(|e| format!("Failed to show settings window: {}", e))?;
    }

    Ok(())
}

// ============================================================================
// Window Management Commands
// ============================================================================

/// Create a new main window with optional layout
#[tauri::command]
fn create_window(
    _app: AppHandle,
    layout_state: State<LayoutState>,
    window_label: String,
    workspace_path: Option<String>,
    initial_layout: Option<WindowLayout>,
) -> Result<String, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.create_main_window(&window_label, workspace_path, initial_layout)
}

/// Close a window and clean up its layout
#[tauri::command]
fn close_window(
    layout_state: State<LayoutState>,
    window_id: String,
) -> Result<(), String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.close_window(&window_id)
}

/// Focus a window
#[tauri::command]
fn focus_window(
    layout_state: State<LayoutState>,
    window_id: String,
) -> Result<(), String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.focus_window(&window_id)
}

/// Get list of all active windows
#[tauri::command]
fn get_window_list(
    layout_state: State<LayoutState>,
) -> Result<Vec<WindowInfo>, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    Ok(window_manager.registry().get_all())
}

// ============================================================================
// Layout Management Commands
// ============================================================================

/// Get layout for a window
#[tauri::command]
fn get_layout(
    layout_state: State<LayoutState>,
    window_id: String,
) -> Result<Option<WindowLayout>, String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.get_layout(&window_id)
}

/// Save layout for a window
#[tauri::command]
fn save_layout_cmd(
    layout_state: State<LayoutState>,
    window_id: String,
    layout: WindowLayout,
) -> Result<(), String> {
    // Ensure window_id matches
    if layout.window_id != window_id {
        return Err("Window ID mismatch".to_string());
    }

    let layout_manager = layout_state.layout_manager.lock().unwrap();

    // Validate layout
    layout_manager.validate_layout(&layout)?;

    // Save layout
    layout_manager.save_layout(layout)
}

/// Update a region size in a layout
#[tauri::command]
fn update_region_size(
    layout_state: State<LayoutState>,
    window_id: String,
    region: String,
    size: u32,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.update_region_size(&window_id, &region, size)
}

/// Toggle a region's visibility
#[tauri::command]
fn toggle_region(
    layout_state: State<LayoutState>,
    window_id: String,
    region: String,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.toggle_region(&window_id, &region)
}

/// Set chat mode for a window
#[tauri::command]
fn set_chat_mode(
    layout_state: State<LayoutState>,
    window_id: String,
    mode: ChatMode,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.set_chat_mode(&window_id, mode)
}

/// Create a detached chat window
#[tauri::command]
fn create_chat_window(
    layout_state: State<LayoutState>,
    source_window_id: String,
) -> Result<String, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.create_chat_window(&source_window_id)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs_pro::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize layout manager
            let db_path = get_layout_db_path(app.handle())
                .map_err(|e| format!("Failed to get layout DB path: {}", e))?;
            let layout_manager = Arc::new(Mutex::new(
                LayoutManager::new(&db_path)
                    .map_err(|e| format!("Failed to create layout manager: {}", e))?
            ));

            // Initialize window manager
            let window_manager = WindowManager::new(
                app.handle().clone(),
                layout_manager.clone(),
            );

            // Store in app state
            app.manage(LayoutState::new(
                layout_manager,
                window_manager,
            ));

            Ok(())
        })
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // File operations
            create_qrate_file,
            open_qrate_file,
            close_qrate_file,
            import_csv_to_qrate,
            // Data operations
            get_rows,
            update_cell,
            add_column,
            update_column,
            insert_row,
            delete_row,
            // Window management (legacy)
            show_main_window,
            show_projects_window,
            show_settings_window,
            // Window management (new)
            create_window,
            close_window,
            focus_window,
            get_window_list,
            create_chat_window,
            // Layout management
            get_layout,
            save_layout_cmd,
            update_region_size,
            toggle_region,
            set_chat_mode,
            // State
            get_current_state,
            // Security
            validate_file_path,
            // Media
            load_image,
            // Settings commands (from settings module)
            settings::commands::get_global_settings_schema,
            settings::commands::get_project_settings_schema,
            settings::commands::get_global_settings_defaults,
            settings::commands::get_project_settings_defaults,
            settings::commands::validate_setting_value,
            settings::commands::get_project_settings,
            settings::commands::set_project_setting,
            settings::commands::set_project_settings,
            settings::commands::get_project_settings_with_defaults_cmd,

        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
