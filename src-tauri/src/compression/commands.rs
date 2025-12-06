use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tauri::{AppHandle, State};

use super::cache::ThumbnailCache;
use super::processor::{spawn_directory_processor, ThumbnailProcessor, ThumbnailProgress};
use crate::app_state::AppState;

pub struct ThumbnailState {
    processors: RwLock<HashMap<String, Arc<ThumbnailProcessor>>>,
    caches: RwLock<HashMap<String, ThumbnailCache>>,
}

impl ThumbnailState {
    pub fn new() -> Self {
        Self {
            processors: RwLock::new(HashMap::new()),
            caches: RwLock::new(HashMap::new()),
        }
    }

    fn get_or_create_cache(
        &self,
        project_path: &std::path::Path,
    ) -> Result<ThumbnailCache, String> {
        let key = project_path.to_string_lossy().to_string();
        if let Some(cache) = self.caches.read().get(&key) {
            return Ok(cache.clone());
        }
        let cache = ThumbnailCache::open(project_path)
            .map_err(|e| format!("Failed to open thumbnail cache: {}", e))?;
        self.caches.write().insert(key, cache.clone());
        Ok(cache)
    }
}

impl Default for ThumbnailState {
    fn default() -> Self {
        Self::new()
    }
}

#[tauri::command]
pub async fn start_thumbnail_processing(
    app_handle: AppHandle,
    thumbnail_state: State<'_, ThumbnailState>,
    app_state: State<'_, AppState>,
    files_folder: String,
) -> Result<ThumbnailProgress, String> {
    let current = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    let project_path = PathBuf::from(&current.path);

    if let Some(processor) = thumbnail_state.processors.read().get(&current.path) {
        if processor.is_processing() {
            return Ok(processor.get_progress());
        }
    }

    let cache = thumbnail_state.get_or_create_cache(&project_path)?;
    let files_dir = PathBuf::from(&files_folder);

    if !files_dir.exists() {
        return Err(format!("Files folder does not exist: {}", files_folder));
    }

    let processor = spawn_directory_processor(cache, files_dir, app_handle);
    thumbnail_state
        .processors
        .write()
        .insert(current.path, Arc::clone(&processor));

    Ok(processor.get_progress())
}

#[tauri::command]
pub async fn cancel_thumbnail_processing(
    thumbnail_state: State<'_, ThumbnailState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let current = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    if let Some(processor) = thumbnail_state.processors.read().get(&current.path) {
        processor.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_thumbnail_path(
    app_state: State<'_, AppState>,
    file_path: String,
) -> Result<String, String> {
    let current = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    let project_path = PathBuf::from(&current.path);
    let source_path = PathBuf::from(&file_path);
    let thumbnails_dir = ThumbnailCache::get_thumbnails_dir(&project_path);
    let hash = ThumbnailCache::compute_hash(&source_path);

    Ok(thumbnails_dir
        .join(format!("{}.webp", hash))
        .to_string_lossy()
        .to_string())
}
