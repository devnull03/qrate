use tauri::State;

use crate::layout::types::{ChatMode, WindowLayout};
use crate::layout_state::LayoutState;

#[tauri::command]
pub fn get_layout(
    layout_state: State<LayoutState>,
    window_id: String,
) -> Result<Option<WindowLayout>, String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.get_layout(&window_id)
}

#[tauri::command]
pub fn save_layout(
    layout_state: State<LayoutState>,
    window_id: String,
    layout: WindowLayout,
) -> Result<(), String> {
    if layout.window_id != window_id {
        return Err("Window ID mismatch".to_string());
    }

    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.validate_layout(&layout)?;
    layout_manager.save_layout(layout)
}

#[tauri::command]
pub fn update_region_size(
    layout_state: State<LayoutState>,
    window_id: String,
    region: String,
    size: u32,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.update_region_size(&window_id, &region, size)
}

#[tauri::command]
pub fn toggle_region(
    layout_state: State<LayoutState>,
    window_id: String,
    region: String,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.toggle_region(&window_id, &region)
}

#[tauri::command]
pub fn set_chat_mode(
    layout_state: State<LayoutState>,
    window_id: String,
    mode: ChatMode,
) -> Result<(), String> {
    let layout_manager = layout_state.layout_manager.lock().unwrap();
    layout_manager.set_chat_mode(&window_id, mode)
}
