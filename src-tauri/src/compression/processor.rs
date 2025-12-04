//! Background thumbnail processor with rayon parallelization.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use super::cache::ThumbnailCache;
use super::is_supported_extension;
use super::pipeline::process_image;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThumbnailProgress {
    pub total: usize,
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub complete: bool,
    pub cancelled: bool,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    path: PathBuf,
    mtime: i64,
}

pub struct ThumbnailProcessor {
    cache: ThumbnailCache,
    total: AtomicUsize,
    processed: AtomicUsize,
    succeeded: AtomicUsize,
    failed: AtomicUsize,
    skipped: AtomicUsize,
    is_processing: AtomicBool,
    cancel_flag: AtomicBool,
    current_file: RwLock<Option<String>>,
    app_handle: Option<AppHandle>,
}

impl ThumbnailProcessor {
    pub fn new(cache: ThumbnailCache) -> Self {
        Self {
            cache,
            total: AtomicUsize::new(0),
            processed: AtomicUsize::new(0),
            succeeded: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            skipped: AtomicUsize::new(0),
            is_processing: AtomicBool::new(false),
            cancel_flag: AtomicBool::new(false),
            current_file: RwLock::new(None),
            app_handle: None,
        }
    }

    pub fn with_app_handle(cache: ThumbnailCache, app_handle: AppHandle) -> Self {
        Self {
            cache,
            total: AtomicUsize::new(0),
            processed: AtomicUsize::new(0),
            succeeded: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            skipped: AtomicUsize::new(0),
            is_processing: AtomicBool::new(false),
            cancel_flag: AtomicBool::new(false),
            current_file: RwLock::new(None),
            app_handle: Some(app_handle),
        }
    }

    pub fn set_app_handle(&mut self, app_handle: AppHandle) {
        self.app_handle = Some(app_handle);
    }

    pub fn get_progress(&self) -> ThumbnailProgress {
        ThumbnailProgress {
            total: self.total.load(Ordering::Relaxed),
            processed: self.processed.load(Ordering::Relaxed),
            succeeded: self.succeeded.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            complete: !self.is_processing.load(Ordering::Relaxed),
            cancelled: self.cancel_flag.load(Ordering::Relaxed),
            current_file: self.current_file.read().clone(),
        }
    }

    pub fn is_processing(&self) -> bool {
        self.is_processing.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    fn reset(&self) {
        self.total.store(0, Ordering::Relaxed);
        self.processed.store(0, Ordering::Relaxed);
        self.succeeded.store(0, Ordering::Relaxed);
        self.failed.store(0, Ordering::Relaxed);
        self.skipped.store(0, Ordering::Relaxed);
        self.cancel_flag.store(false, Ordering::Relaxed);
        *self.current_file.write() = None;
    }

    fn emit_progress(&self) {
        if let Some(ref app_handle) = self.app_handle {
            let _ = app_handle.emit("thumbnail-progress", self.get_progress());
        }
    }

    fn emit_complete(&self) {
        if let Some(ref app_handle) = self.app_handle {
            let _ = app_handle.emit("thumbnail-complete", self.get_progress());
        }
    }

    pub fn scan_directory(&self, dir: &Path) -> Vec<FileInfo> {
        WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|entry| {
                let path = entry.path();
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();

                if !is_supported_extension(&ext) {
                    return None;
                }

                let mtime = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                Some(FileInfo {
                    path: path.to_path_buf(),
                    mtime,
                })
            })
            .collect()
    }

    pub fn scan_paths(&self, paths: &[PathBuf]) -> Vec<FileInfo> {
        paths
            .iter()
            .filter(|path| path.is_file())
            .filter_map(|path| {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();

                if !is_supported_extension(&ext) {
                    return None;
                }

                let mtime = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                Some(FileInfo {
                    path: path.clone(),
                    mtime,
                })
            })
            .collect()
    }

    fn filter_uncached(&self, files: Vec<FileInfo>) -> Vec<FileInfo> {
        let items: Vec<(PathBuf, i64)> = files.iter().map(|f| (f.path.clone(), f.mtime)).collect();

        let cached_hashes = self.cache.batch_check_valid(&items).unwrap_or_default();

        files
            .into_iter()
            .filter(|f| !cached_hashes.contains(&ThumbnailCache::compute_hash(&f.path)))
            .collect()
    }

    pub fn process_directory(&self, dir: &Path) -> ThumbnailProgress {
        if self.is_processing.load(Ordering::Relaxed) {
            return self.get_progress();
        }

        self.reset();
        self.is_processing.store(true, Ordering::Relaxed);

        let all_files = self.scan_directory(dir);
        let total_scanned = all_files.len();
        let files_to_process = self.filter_uncached(all_files);
        let skipped = total_scanned - files_to_process.len();

        self.total.store(total_scanned, Ordering::Relaxed);
        self.skipped.store(skipped, Ordering::Relaxed);
        self.processed.store(skipped, Ordering::Relaxed);
        self.emit_progress();

        self.process_batch(&files_to_process);

        self.is_processing.store(false, Ordering::Relaxed);
        *self.current_file.write() = None;
        self.emit_complete();
        self.get_progress()
    }

    pub fn process_paths(&self, paths: &[PathBuf]) -> ThumbnailProgress {
        if self.is_processing.load(Ordering::Relaxed) {
            return self.get_progress();
        }

        self.reset();
        self.is_processing.store(true, Ordering::Relaxed);

        let all_files = self.scan_paths(paths);
        let total_scanned = all_files.len();
        let files_to_process = self.filter_uncached(all_files);
        let skipped = total_scanned - files_to_process.len();

        self.total.store(total_scanned, Ordering::Relaxed);
        self.skipped.store(skipped, Ordering::Relaxed);
        self.processed.store(skipped, Ordering::Relaxed);
        self.emit_progress();

        self.process_batch(&files_to_process);

        self.is_processing.store(false, Ordering::Relaxed);
        *self.current_file.write() = None;
        self.emit_complete();
        self.get_progress()
    }

    fn process_batch(&self, files: &[FileInfo]) {
        let cache = self.cache.clone();

        files.par_iter().for_each(|file| {
            if self.cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            *self.current_file.write() = Some(
                file.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            );

            match process_image(&file.path) {
                Ok(result) => {
                    if cache
                        .store_thumbnail(
                            &file.path,
                            file.mtime,
                            result.original_width,
                            result.original_height,
                            result.thumb_width,
                            result.thumb_height,
                            &result.webp_data,
                        )
                        .is_ok()
                    {
                        self.succeeded.fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    self.failed.fetch_add(1, Ordering::Relaxed);
                }
            }

            self.processed.fetch_add(1, Ordering::Relaxed);

            if self.processed.load(Ordering::Relaxed).is_multiple_of(10) {
                self.emit_progress();
            }
        });
    }

    pub fn cache(&self) -> &ThumbnailCache {
        &self.cache
    }
}

pub fn spawn_directory_processor(
    cache: ThumbnailCache,
    dir: PathBuf,
    app_handle: AppHandle,
) -> Arc<ThumbnailProcessor> {
    let processor = Arc::new(ThumbnailProcessor::with_app_handle(cache, app_handle));
    let processor_clone = Arc::clone(&processor);

    std::thread::spawn(move || {
        processor_clone.process_directory(&dir);
    });

    processor
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_progress_tracking() {
        let dir = tempdir().unwrap();
        let qrate_path = dir.path().join("test.qrate");
        fs::write(&qrate_path, "").unwrap();

        let cache = ThumbnailCache::open(&qrate_path).unwrap();
        let processor = ThumbnailProcessor::new(cache);

        let progress = processor.get_progress();
        assert_eq!(progress.total, 0);
        assert!(!processor.is_processing());
    }

    #[test]
    fn test_cancel_flag() {
        let dir = tempdir().unwrap();
        let qrate_path = dir.path().join("test.qrate");
        fs::write(&qrate_path, "").unwrap();

        let cache = ThumbnailCache::open(&qrate_path).unwrap();
        let processor = ThumbnailProcessor::new(cache);

        assert!(!processor.cancel_flag.load(Ordering::Relaxed));
        processor.cancel();
        assert!(processor.cancel_flag.load(Ordering::Relaxed));
    }
}
