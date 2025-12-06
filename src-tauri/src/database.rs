use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::settings;

pub const QRATE_FOLDER_NAME: &str = ".qrate";
pub const PROJECT_CONFIG_NAME: &str = "project.json";
pub const DB_FILE_NAME: &str = "db.sqlite";

/// Project configuration stored in .qrate/project.json
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectConfig {
    pub version: String,
    pub name: String,
    pub created_at: String,
    pub mode: ProjectMode,
    pub paths: ProjectPaths,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectMode {
    Reference,
    Copy,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectPaths {
    /// Absolute path if Reference mode, relative to project if Copy mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images_root: Option<String>,
    /// Relative to .qrate folder
    pub database: String,
    /// Original CSV file copied into project (relative to .qrate folder)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_csv: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            name: "Untitled Project".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            mode: ProjectMode::Reference,
            paths: ProjectPaths {
                images_root: None,
                database: format!("./{}", DB_FILE_NAME),
                source_csv: None,
            },
        }
    }
}

impl ProjectConfig {
    pub fn new(name: String, mode: ProjectMode, images_root: Option<String>) -> Self {
        Self {
            version: "1.0.0".to_string(),
            name,
            created_at: chrono::Utc::now().to_rfc3339(),
            mode,
            paths: ProjectPaths {
                images_root,
                database: format!("./{}", DB_FILE_NAME),
                source_csv: None,
            },
        }
    }
}

pub fn get_config_path(project_path: &Path) -> PathBuf {
    get_qrate_folder(project_path).join(PROJECT_CONFIG_NAME)
}

pub fn load_project_config(project_path: &Path) -> std::io::Result<ProjectConfig> {
    let config_path = get_config_path(project_path);
    let content = std::fs::read_to_string(&config_path)?;
    serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn save_project_config(project_path: &Path, config: &ProjectConfig) -> std::io::Result<()> {
    let config_path = get_config_path(project_path);
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&config_path, content)
}

pub fn get_qrate_folder(project_path: &Path) -> PathBuf {
    project_path.join(QRATE_FOLDER_NAME)
}

pub fn get_db_path(project_path: &Path) -> PathBuf {
    get_qrate_folder(project_path).join(DB_FILE_NAME)
}

pub fn get_thumbnails_dir(project_path: &Path) -> PathBuf {
    get_qrate_folder(project_path).join("thumbnails")
}

pub fn is_qrate_project(path: &Path) -> bool {
    get_config_path(path).exists()
}

/// Copy a CSV file into the project root (beside .qrate folder) and return the relative path
pub fn copy_csv_to_project(project_path: &Path, csv_path: &Path) -> std::io::Result<String> {
    let file_name = csv_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("source.csv"));
    let dest_path = project_path.join(file_name);

    std::fs::copy(csv_path, &dest_path)?;

    Ok(file_name.to_string_lossy().to_string())
}

fn ensure_qrate_folder(project_path: &Path) -> std::io::Result<PathBuf> {
    let folder = get_qrate_folder(project_path);
    std::fs::create_dir_all(&folder)?;
    std::fs::create_dir_all(get_thumbnails_dir(project_path))?;
    Ok(folder)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnDef {
    pub id: String,
    pub name: String,
    pub col_type: String,
    pub width: i32,
    pub hidden: bool,
}

fn setup_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -64000;
         PRAGMA temp_store = MEMORY;",
    )
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS _settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS _columns (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            type TEXT NOT NULL DEFAULT 'Text',
            width INTEGER NOT NULL DEFAULT 150,
            hidden INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS data (
            row_id INTEGER PRIMARY KEY AUTOINCREMENT
        );
        CREATE INDEX IF NOT EXISTS idx_data_row_id ON data(row_id);
        CREATE TABLE IF NOT EXISTS _annotations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            row_id INTEGER,
            column_id TEXT,
            severity TEXT DEFAULT 'info',
            message TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            resolved INTEGER NOT NULL DEFAULT 0,
            metadata TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_annotations_provider ON _annotations(provider);
        CREATE INDEX IF NOT EXISTS idx_annotations_location ON _annotations(row_id, column_id);
        CREATE TABLE IF NOT EXISTS _thumbnails (
            hash TEXT PRIMARY KEY,
            source_path TEXT NOT NULL,
            source_mtime INTEGER NOT NULL,
            original_width INTEGER NOT NULL,
            original_height INTEGER NOT NULL,
            thumb_width INTEGER NOT NULL,
            thumb_height INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_thumbnails_source_path ON _thumbnails(source_path);
        INSERT OR IGNORE INTO _meta (key, value) VALUES ('version', '1.0');
        INSERT OR IGNORE INTO _meta (key, value) VALUES ('created_at', datetime('now'));",
    )
}

pub fn init_database(project_path: &Path) -> Result<Connection> {
    init_database_with_config(project_path, None)
}

pub fn init_database_with_config(
    project_path: &Path,
    config: Option<ProjectConfig>,
) -> Result<Connection> {
    ensure_qrate_folder(project_path).map_err(|e| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!(
            "Failed to create .qrate folder: {}",
            e
        )))
    })?;

    // Create project.json config
    let config = config.unwrap_or_else(|| {
        let name = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled Project")
            .to_string();
        ProjectConfig::new(name, ProjectMode::Reference, None)
    });

    save_project_config(project_path, &config).map_err(|e| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!(
            "Failed to create project.json: {}",
            e
        )))
    })?;

    let conn = Connection::open(get_db_path(project_path))?;
    setup_pragmas(&conn)?;
    create_schema(&conn)?;
    settings::init_project_settings(&conn)?;
    Ok(conn)
}

pub fn open_database(project_path: &Path) -> Result<Connection> {
    let _config = load_project_config(project_path).map_err(|e| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!("Not a valid qrate project: {}", e)))
    })?;

    let db_path = get_db_path(project_path);
    if !db_path.exists() {
        return Err(rusqlite::Error::InvalidPath(PathBuf::from(format!(
            "Database not found: {}",
            db_path.display()
        ))));
    }

    let conn = Connection::open(&db_path)?;
    setup_pragmas(&conn)?;
    settings::ensure_project_settings(&conn)?;
    Ok(conn)
}

pub fn get_columns(conn: &Connection) -> Result<Vec<ColumnDef>> {
    let mut stmt =
        conn.prepare("SELECT id, name, type, width, hidden FROM _columns ORDER BY ROWID")?;
    let cols = stmt
        .query_map([], |row| {
            Ok(ColumnDef {
                id: row.get(0)?,
                name: row.get(1)?,
                col_type: row.get(2)?,
                width: row.get(3)?,
                hidden: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(cols)
}

pub fn add_column(conn: &Connection, column: &ColumnDef, default_value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO _columns (id, name, type, width, hidden) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            column.id,
            column.name,
            column.col_type,
            column.width,
            column.hidden as i32
        ],
    )?;
    conn.execute(
        &format!(
            "ALTER TABLE data ADD COLUMN [{}] TEXT DEFAULT '{}'",
            column.id, default_value
        ),
        [],
    )?;
    Ok(())
}

pub fn update_column(conn: &Connection, column: &ColumnDef) -> Result<()> {
    conn.execute(
        "UPDATE _columns SET name = ?1, type = ?2, width = ?3, hidden = ?4 WHERE id = ?5",
        params![
            column.name,
            column.col_type,
            column.width,
            column.hidden as i32,
            column.id
        ],
    )?;
    Ok(())
}

pub fn get_rows(conn: &Connection, limit: u32, offset: u32) -> Result<Vec<serde_json::Value>> {
    let columns = get_columns(conn)?;
    if columns.is_empty() {
        return Ok(vec![]);
    }

    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let query = format!(
        "SELECT row_id, {} FROM data ORDER BY row_id LIMIT {} OFFSET {}",
        col_names.join(", "),
        limit,
        offset
    );

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt
        .query_map([], |row| {
            let mut map = serde_json::Map::new();
            map.insert("row_id".into(), serde_json::json!(row.get::<_, i64>(0)?));
            for (idx, col) in columns.iter().enumerate() {
                let val: Option<String> = row.get(idx + 1)?;
                map.insert(
                    col.id.clone(),
                    serde_json::Value::String(val.unwrap_or_default()),
                );
            }
            Ok(serde_json::Value::Object(map))
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_row_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM data", [], |row| row.get(0))
}

pub fn update_cell(conn: &Connection, row_id: i64, column_id: &str, value: &str) -> Result<()> {
    conn.execute(
        &format!("UPDATE data SET [{}] = ?1 WHERE row_id = ?2", column_id),
        params![value, row_id],
    )?;
    Ok(())
}

pub fn insert_row(
    conn: &Connection,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<i64> {
    let columns = get_columns(conn)?;
    if columns.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "INSERT INTO data ({}) VALUES ({})",
        col_names.join(", "),
        placeholders.join(", ")
    );

    let str_values: Vec<String> = columns
        .iter()
        .map(|col| {
            values
                .get(&col.id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let params: Vec<&dyn rusqlite::ToSql> = str_values
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();

    conn.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_row(conn: &Connection, row_id: i64) -> Result<()> {
    conn.execute("DELETE FROM data WHERE row_id = ?1", params![row_id])?;
    Ok(())
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO _settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_all_settings(conn: &Connection) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM _settings")?;
    let mut settings = std::collections::HashMap::new();
    for row in stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (key, value) = row?;
        settings.insert(key, value);
    }
    Ok(settings)
}

pub fn set_all_settings(
    conn: &Connection,
    settings: &std::collections::HashMap<String, String>,
) -> Result<()> {
    for (key, value) in settings {
        set_setting(conn, key, value)?;
    }
    Ok(())
}

pub fn import_csv_data(
    conn: &Connection,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
) -> Result<()> {
    conn.execute("DELETE FROM data", [])?;
    conn.execute("DELETE FROM _columns", [])?;

    for (idx, header) in headers.iter().enumerate() {
        let column = ColumnDef {
            id: format!("col_{}", idx),
            name: header.clone(),
            col_type: "Text".into(),
            width: 150,
            hidden: false,
        };
        add_column(conn, &column, "")?;
    }

    let columns = get_columns(conn)?;
    let col_names: Vec<String> = columns.iter().map(|c| format!("[{}]", c.id)).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "INSERT INTO data ({}) VALUES ({})",
        col_names.join(", "),
        placeholders.join(", ")
    );

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(&sql)?;
        for row_values in rows {
            let params: Vec<&dyn rusqlite::ToSql> = row_values
                .iter()
                .map(|v| v as &dyn rusqlite::ToSql)
                .collect();
            stmt.execute(rusqlite::params_from_iter(params.iter()))?;
        }
    }
    tx.commit()?;
    Ok(())
}

// Thumbnail functions (merged from cache.rs)

pub fn compute_thumbnail_hash(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    blake3::hash(normalized.as_bytes()).to_hex().to_string()
}

#[allow(clippy::too_many_arguments)]
pub fn store_thumbnail(
    conn: &Connection,
    thumbnails_dir: &Path,
    source_path: &Path,
    source_mtime: i64,
    original_width: u32,
    original_height: u32,
    thumb_width: u32,
    thumb_height: u32,
    webp_data: &[u8],
) -> Result<PathBuf> {
    let hash = compute_thumbnail_hash(source_path);
    let thumb_path = thumbnails_dir.join(format!("{}.webp", hash));

    std::fs::write(&thumb_path, webp_data).map_err(|e| {
        rusqlite::Error::InvalidPath(PathBuf::from(format!("Failed to write thumbnail: {}", e)))
    })?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT OR REPLACE INTO _thumbnails
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

pub fn batch_check_thumbnails_valid(
    conn: &Connection,
    thumbnails_dir: &Path,
    items: &[(PathBuf, i64)],
) -> Result<HashSet<String>> {
    let mut stmt =
        conn.prepare_cached("SELECT 1 FROM _thumbnails WHERE hash = ?1 AND source_mtime = ?2")?;
    let mut result = HashSet::new();

    for (path, mtime) in items {
        let hash = compute_thumbnail_hash(path);
        let thumb_path = thumbnails_dir.join(format!("{}.webp", hash));
        if thumb_path.exists() && stmt.exists(params![&hash, mtime])? {
            result.insert(hash);
        }
    }
    Ok(result)
}
