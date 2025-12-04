//! Thumbnail cache module - file-based storage for thumbnails with SQLite metadata.

use rusqlite::{params, Connection, OptionalExtension, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailMetadata {
    pub hash: String,
    pub source_path: String,
    pub source_mtime: i64,
    pub original_width: u32,
    pub original_height: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub file_size: usize,
    pub created_at: i64,
}

pub struct ThumbnailCache {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
    thumbnails_dir: PathBuf,
}

impl ThumbnailCache {
    pub fn get_project_folder(qrate_path: &Path) -> PathBuf {
        let parent = qrate_path.parent().unwrap_or(Path::new("."));
        let stem = qrate_path.file_stem().unwrap_or_default().to_string_lossy();
        parent.join(format!(".{}.qrate", stem))
    }

    pub fn get_db_path(qrate_path: &Path) -> PathBuf {
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
            db_path,
            thumbnails_dir,
        })
    }

    pub fn compute_hash(path: &Path) -> String {
        blake3::hash(path.to_string_lossy().as_bytes())
            .to_hex()
            .to_string()
    }

    pub fn get_thumbnail_path(&self, source_path: &Path) -> PathBuf {
        let hash = Self::compute_hash(source_path);
        self.thumbnails_dir.join(format!("{}.webp", hash))
    }

    pub fn has_valid_thumbnail(&self, path: &Path, mtime: i64) -> SqliteResult<bool> {
        let hash = Self::compute_hash(path);
        let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

        if !thumb_path.exists() {
            return Ok(false);
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT 1 FROM thumbnails WHERE hash = ?1 AND source_mtime = ?2 LIMIT 1",
        )?;
        stmt.exists(params![hash, mtime])
    }

    pub fn has_thumbnail(&self, path: &Path) -> SqliteResult<bool> {
        let hash = Self::compute_hash(path);
        let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

        if !thumb_path.exists() {
            return Ok(false);
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT 1 FROM thumbnails WHERE hash = ?1 LIMIT 1")?;
        stmt.exists(params![hash])
    }

    pub fn get_metadata(&self, path: &Path) -> SqliteResult<Option<ThumbnailMetadata>> {
        let hash = Self::compute_hash(path);
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare_cached(
            "SELECT hash, source_path, source_mtime, original_width, original_height,
                    thumb_width, thumb_height, file_size, created_at
             FROM thumbnails WHERE hash = ?1",
        )?;

        stmt.query_row(params![hash], |row| {
            Ok(ThumbnailMetadata {
                hash: row.get(0)?,
                source_path: row.get(1)?,
                source_mtime: row.get(2)?,
                original_width: row.get(3)?,
                original_height: row.get(4)?,
                thumb_width: row.get(5)?,
                thumb_height: row.get(6)?,
                file_size: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .optional()
    }

    pub fn get_thumbnail_file_path(&self, source_path: &Path) -> Option<PathBuf> {
        let hash = Self::compute_hash(source_path);
        let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

        if thumb_path.exists() {
            Some(thumb_path)
        } else {
            None
        }
    }

    pub fn get_thumbnail_bytes(&self, path: &Path) -> SqliteResult<Option<Vec<u8>>> {
        let thumb_path = match self.get_thumbnail_file_path(path) {
            Some(p) => p,
            None => return Ok(None),
        };

        match std::fs::read(&thumb_path) {
            Ok(data) => Ok(Some(data)),
            Err(_) => Ok(None),
        }
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

    pub fn remove_thumbnail(&self, path: &Path) -> SqliteResult<bool> {
        let hash = Self::compute_hash(path);
        let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

        let _ = std::fs::remove_file(&thumb_path);

        let affected = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM thumbnails WHERE hash = ?1", params![hash])?;
        Ok(affected > 0)
    }

    pub fn cleanup_orphaned(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare("SELECT hash, source_path FROM thumbnails")?;
        let entries: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        let mut removed = 0;
        for (hash, source_path) in entries {
            let thumb_path = self.thumbnails_dir.join(format!("{}.webp", hash));

            if !Path::new(&source_path).exists() {
                let _ = std::fs::remove_file(&thumb_path);
                conn.execute("DELETE FROM thumbnails WHERE hash = ?1", params![hash])?;
                removed += 1;
            } else if !thumb_path.exists() {
                conn.execute("DELETE FROM thumbnails WHERE hash = ?1", params![hash])?;
                removed += 1;
            }
        }

        Ok(removed)
    }

    pub fn count(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM thumbnails", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn total_size(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(file_size), 0) FROM thumbnails",
            [],
            |row| row.get(0),
        )?;
        Ok(size as usize)
    }

    pub fn clear(&self) -> SqliteResult<()> {
        if let Ok(entries) = std::fs::read_dir(&self.thumbnails_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "webp").unwrap_or(false) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }

        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM thumbnails", [])?;
        Ok(())
    }

    pub fn checkpoint(&self) -> SqliteResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn thumbnails_dir(&self) -> &Path {
        &self.thumbnails_dir
    }

    pub fn get_all_hashes(&self) -> SqliteResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT hash FROM thumbnails")?;
        let hashes = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(hashes)
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
            db_path: self.db_path.clone(),
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
        assert!(cache.has_thumbnail(&source_path).unwrap());
        assert!(cache.has_valid_thumbnail(&source_path, 12345).unwrap());
        assert!(!cache.has_valid_thumbnail(&source_path, 99999).unwrap());

        let meta = cache.get_metadata(&source_path).unwrap().unwrap();
        assert_eq!(meta.original_width, 1920);
        assert_eq!(meta.thumb_width, 300);

        assert_eq!(cache.count().unwrap(), 1);

        assert!(cache.remove_thumbnail(&source_path).unwrap());
        assert!(!cache.has_thumbnail(&source_path).unwrap());
        assert!(!thumb_path.exists());
    }

    #[test]
    fn test_thumbnail_file_path() {
        let dir = tempdir().unwrap();
        let qrate_path = dir.path().join("test.qrate");
        fs::write(&qrate_path, "").unwrap();

        let cache = ThumbnailCache::open(&qrate_path).unwrap();
        let source_path = Path::new("/test/image.jpg");

        let thumb_path = cache.get_thumbnail_path(source_path);
        assert!(thumb_path.to_string_lossy().ends_with(".webp"));
        assert!(thumb_path.starts_with(cache.thumbnails_dir()));
    }
}
