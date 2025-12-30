use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

mod annotations;
mod app_state;
mod checks;
mod compression;
mod database;
mod file;
mod layout;
mod settings;
mod window;

use app_state::AppState;
use checks::spellcheck::SpellCheckState;
use compression::commands::ThumbnailState;
use layout::{manager::LayoutManager, persistence::get_layout_db_path, LayoutState};
use window::manager::WindowManager;

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs_pro::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.app_handle().webview_windows().len() == 1 {
                    if let Some(app_state) = window.try_state::<AppState>() {
                        for entry in app_state.connections.iter() {
                            if let Ok(conn) = entry.value().lock() {
                                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                            }
                        }
                    }
                }
            }
        })
        .setup(|app| {
            let db_path = get_layout_db_path(app.handle()).map_err(|e| e.to_string())?;
            let layout_manager = Arc::new(Mutex::new(
                LayoutManager::new(&db_path).map_err(|e| e.to_string())?,
            ));
            let window_manager = WindowManager::new(app.handle().clone(), layout_manager.clone());
            app.manage(LayoutState::new(layout_manager, window_manager));

            if let Some(project_path) = std::env::args().nth(1) {
                if database::is_qrate_project(std::path::Path::new(&project_path)) {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let _ = handle.emit("open-project", project_path);
                    });
                }
            }
            Ok(())
        })
        .manage(AppState::new())
        .manage(SpellCheckState::new())
        .manage(ThumbnailState::new())
        .invoke_handler(tauri::generate_handler![
            file::commands::create_qrate_project,
            file::commands::open_qrate_project,
            file::commands::close_qrate_project,
            file::commands::import_csv_to_qrate,
            file::commands::preview_csv,
            file::commands::get_rows,
            file::commands::update_cell,
            file::commands::add_column,
            file::commands::update_column,
            file::commands::insert_row,
            file::commands::delete_row,
            file::commands::get_current_state,
            file::commands::validate_file_path,
            window::commands::show_main_window,
            window::commands::show_projects_window,
            window::commands::show_settings_window,
            window::commands::create_window,
            window::commands::close_window,
            window::commands::focus_window,
            window::commands::get_window_list,
            window::commands::create_chat_window,
            layout::commands::get_layout,
            layout::commands::save_layout,
            layout::commands::update_region_size,
            layout::commands::toggle_region,
            layout::commands::set_chat_mode,
            compression::commands::start_thumbnail_processing,
            compression::commands::cancel_thumbnail_processing,
            compression::commands::get_thumbnail_path,
            settings::commands::get_global_settings_schema,
            settings::commands::get_project_settings_schema,
            settings::commands::get_global_settings_defaults,
            settings::commands::get_project_settings_defaults,
            settings::commands::validate_setting_value,
            settings::commands::get_project_settings,
            settings::commands::set_project_setting,
            settings::commands::set_project_settings,
            settings::commands::get_project_settings_with_defaults_cmd,
            checks::spellcheck::check_spelling,
            checks::spellcheck::check_text_fragment,
            checks::spellcheck::get_suggestions,
            checks::spellcheck::ignore_word,
            checks::spellcheck::add_to_dictionary,
            checks::spellcheck::is_word_correct,
            checks::spellcheck::load_dictionary_file,
            annotations::commands::get_annotations,
            annotations::commands::create_annotation,
            annotations::commands::update_annotation,
            annotations::commands::delete_annotation,
            annotations::commands::get_annotations_at,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
