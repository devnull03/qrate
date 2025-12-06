use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::database;

pub struct ThumbnailCache {
    conn: Arc<Mutex<Connection>>,
    thumbnails_dir: PathBuf,
}

impl ThumbnailCache {
    pub fn get_thumbnails_dir(project_path: &Path) -> PathBuf {
        database::get_thumbnails_dir(project_path)
    }

    pub fn open(project_path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = database::open_database(project_path)?;
        let thumbnails_dir = database::get_thumbnails_dir(project_path);
        std::fs::create_dir_all(&thumbnails_dir).ok();

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            thumbnails_dir,
        })
    }

    pub fn compute_hash(path: &Path) -> String {
        database::compute_thumbnail_hash(path)
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
    ) -> Result<PathBuf, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        database::store_thumbnail(
            &conn,
            &self.thumbnails_dir,
            source_path,
            source_mtime,
            original_width,
            original_height,
            thumb_width,
            thumb_height,
            webp_data,
        )
    }

    pub fn batch_check_valid(
        &self,
        items: &[(PathBuf, i64)],
    ) -> Result<HashSet<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        database::batch_check_thumbnails_valid(&conn, &self.thumbnails_dir, items)
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
