//! Thumbnail cache module - file-based storage for thumbnails with SQLite metadata.

use rusqlite::{params, Connection, Result as SqliteResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct ThumbnailCache {
    conn: Arc<Mutex<Connection>>,
    thumbnails_dir: PathBuf,
}

impl ThumbnailCache {
    fn get_project_folder(qrate_path: &Path) -> PathBuf {
        let parent = qrate_path.parent().unwrap_or(Path::new("."));
        let stem = qrate_path.file_stem().unwrap_or_default().to_string_lossy();
        parent.join(format!(".{}.qrate", stem))
    }

    fn get_db_path(qrate_path: &Path) -> PathBuf {
        Self::get_project_folder(qrate_path).join("thumbnails.db")
    }

    pub fn get_thumbnails_dir(qrate_path: &Path) -> PathBuf {
        Self::get_project_folder(qrate_path).join("thumbnails")
    }

    pub fn open(qrate_path: &Path) -> SqliteResult<Self> {
        let db_path = Self::get_db_path(qrate_path);
        let thumbnails_dir = Self::get_thumbnails_dir(qrate_path);

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::InvalidPath(PathBuf::from(format!(
                    "Failed to create cache directory: {}",
                    e
                )))
            })?;
        }

        std::fs::create_dir_all(&thumbnails_dir).map_err(|e| {
            rusqlite::Error::InvalidPath(PathBuf::from(format!(
                "Failed to create thumbnails directory: {}",
                e
            )))
        })?;

        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;
             PRAGMA temp_store = MEMORY;",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS thumbnails (
                hash TEXT PRIMARY KEY,
                source_path TEXT NOT NULL,
                source_mtime INTEGER NOT NULL,
                original_width INTEGER NOT NULL,
                original_height INTEGER NOT NULL,
                thumb_width INTEGER NOT NULL,
                thumb_height INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_thumbnails_source_path ON thumbnails(source_path)",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            thumbnails_dir,
        })
    }

    pub fn compute_hash(path: &Path) -> String {
        let normalized = path.to_string_lossy().replace('\\', "/");
        blake3::hash(normalized.as_bytes()).to_hex().to_string()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store_thumbnail(
        &self,
        source_path: &Path,
        source_mtime: i64,
        original_width: u32,
        original_height: u32,
        thumb_width: u32,
        thumb_height: u32,
        webp_data: &[u8],
    ) -> SqliteResult<PathBuf> {
        let hash = Self::compute_hash(source_path);
        let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

        std::fs::write(&thumb_path, webp_data).map_err(|e| {
            rusqlite::Error::InvalidPath(PathBuf::from(format!(
                "Failed to write thumbnail file: {}",
                e
            )))
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO thumbnails
             (hash, source_path, source_mtime, original_width, original_height,
              thumb_width, thumb_height, file_size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                hash,
                source_path.to_string_lossy().to_string(),
                source_mtime,
                original_width,
                original_height,
                thumb_width,
                thumb_height,
                webp_data.len(),
                now,
            ],
        )?;

        Ok(thumb_path)
    }

    pub fn batch_check_valid(&self, items: &[(PathBuf, i64)]) -> SqliteResult<HashSet<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("SELECT 1 FROM thumbnails WHERE hash = ?1 AND source_mtime = ?2")?;

        let mut result = HashSet::new();
        for (path, mtime) in items {
            let hash = Self::compute_hash(path);
            let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

            if thumb_path.exists() && stmt.exists(params![&hash, mtime])? {
                result.insert(hash);
            }
        }
        Ok(result)
    }
}

impl Clone for ThumbnailCache {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            thumbnails_dir: self.thumbnails_dir.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_cache_operations() {
        let dir = tempdir().unwrap();
        let qrate_path = dir.path().join("test.qrate");
        fs::write(&qrate_path, "").unwrap();

        let cache = ThumbnailCache::open(&qrate_path).unwrap();
        let source_path = dir.path().join("image.jpg");
        fs::write(&source_path, "fake image data").unwrap();

        let webp_data = vec![0u8; 100];

        let thumb_path = cache
            .store_thumbnail(&source_path, 12345, 1920, 1080, 300, 169, &webp_data)
            .unwrap();

        assert!(thumb_path.exists());
    }

    #[test]
    fn test_batch_check_valid() {
        let dir = tempdir().unwrap();
        let qrate_path = dir.path().join("test.qrate");
        fs::write(&qrate_path, "").unwrap();

        let cache = ThumbnailCache::open(&qrate_path).unwrap();
        let source_path = dir.path().join("image.jpg");
        fs::write(&source_path, "fake image data").unwrap();

        let webp_data = vec![0u8; 100];
        cache
            .store_thumbnail(&source_path, 12345, 1920, 1080, 300, 169, &webp_data)
            .unwrap();

        let items = vec![(source_path.clone(), 12345i64)];
        let valid = cache.batch_check_valid(&items).unwrap();
        assert_eq!(valid.len(), 1);

        let items_wrong_mtime = vec![(source_path, 99999i64)];
        let valid = cache.batch_check_valid(&items_wrong_mtime).unwrap();
        assert_eq!(valid.len(), 0);
    }
}
