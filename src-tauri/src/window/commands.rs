use tauri::{AppHandle, Manager, State};

use crate::layout::types::WindowLayout;
use crate::layout_state::LayoutState;
use crate::window::registry::WindowInfo;

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

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

#[tauri::command]
pub fn show_projects_window(app: AppHandle) -> Result<(), String> {
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

#[tauri::command]
pub fn show_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(settings_window) = app.get_webview_window("settings") {
        settings_window
            .show()
            .map_err(|e| format!("Failed to show settings window: {}", e))?;
        settings_window
            .set_focus()
            .map_err(|e| format!("Failed to focus settings window: {}", e))?;
    } else {
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

#[tauri::command]
pub fn create_window(
    _app: AppHandle,
    layout_state: State<LayoutState>,
    window_label: String,
    workspace_path: Option<String>,
    initial_layout: Option<WindowLayout>,
) -> Result<String, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.create_main_window(&window_label, workspace_path, initial_layout)
}

#[tauri::command]
pub fn close_window(layout_state: State<LayoutState>, window_id: String) -> Result<(), String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.close_window(&window_id)
}

#[tauri::command]
pub fn focus_window(layout_state: State<LayoutState>, window_id: String) -> Result<(), String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.focus_window(&window_id)
}

#[tauri::command]
pub fn get_window_list(layout_state: State<LayoutState>) -> Result<Vec<WindowInfo>, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    Ok(window_manager.registry().get_all())
}

#[tauri::command]
pub fn create_chat_window(
    layout_state: State<LayoutState>,
    source_window_id: String,
) -> Result<String, String> {
    let window_manager = layout_state.window_manager.lock().unwrap();
    window_manager.create_chat_window(&source_window_id)
}
