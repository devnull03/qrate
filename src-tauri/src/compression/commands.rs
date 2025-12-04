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

    pub fn get_or_create_cache(
        &self,
        qrate_path: &std::path::Path,
    ) -> Result<ThumbnailCache, String> {
        let key = qrate_path.to_string_lossy().to_string();

        if let Some(cache) = self.caches.read().get(&key) {
            return Ok(cache.clone());
        }

        let cache = ThumbnailCache::open(qrate_path)
            .map_err(|e| format!("Failed to open thumbnail cache: {}", e))?;

        self.caches.write().insert(key, cache.clone());
        Ok(cache)
    }

    fn get_processor(&self, qrate_path: &str) -> Option<Arc<ThumbnailProcessor>> {
        self.processors.read().get(qrate_path).cloned()
    }

    fn set_processor(&self, qrate_path: String, processor: Arc<ThumbnailProcessor>) {
        self.processors.write().insert(qrate_path, processor);
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
    let current_file = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    let qrate_path = PathBuf::from(&current_file.path);
    let key = current_file.path.clone();

    if let Some(processor) = thumbnail_state.get_processor(&key) {
        if processor.is_processing() {
            return Ok(processor.get_progress());
        }
    }

    let cache = thumbnail_state.get_or_create_cache(&qrate_path)?;
    let files_dir = PathBuf::from(&files_folder);

    if !files_dir.exists() {
        return Err(format!("Files folder does not exist: {}", files_folder));
    }

    let processor = spawn_directory_processor(cache, files_dir, app_handle);
    thumbnail_state.set_processor(key, Arc::clone(&processor));

    Ok(processor.get_progress())
}

#[tauri::command]
pub async fn cancel_thumbnail_processing(
    thumbnail_state: State<'_, ThumbnailState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let current_file = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    if let Some(processor) = thumbnail_state.get_processor(&current_file.path) {
        processor.cancel();
    }

    Ok(())
}

#[tauri::command]
pub async fn get_thumbnail_path(
    app_state: State<'_, AppState>,
    file_path: String,
) -> Result<String, String> {
    let current_file = app_state
        .get_current_file()
        .ok_or("No project is currently open")?;

    let qrate_path = PathBuf::from(&current_file.path);
    let source_path = PathBuf::from(&file_path);
    let thumbnails_dir = ThumbnailCache::get_thumbnails_dir(&qrate_path);
    let hash = ThumbnailCache::compute_hash(&source_path);
    let thumb_path = thumbnails_dir.join(format!("{}.webp", hash));

    Ok(thumb_path.to_string_lossy().to_string())
}
