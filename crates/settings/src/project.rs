//! On-disk `.qrate` project file (v1). One SQLite database per project,
//! written once by the "New Project" wizard. WAL behavior was validated by
//! `examples/qrate_wal_probe.rs` (ASNT-16): a clean close with zero remaining
//! connections removes the `-wal`/`-shm` siblings, and the explicit
//! `wal_checkpoint(TRUNCATE)` before close is cheap insurance.
//!
//! v1 schema — only what has a consumer today:
//! - `__settings`  key/value: project name, source kind, link method, created_at.
//! - `__columns`   configured columns (`notes` is where ASNT-18's per-column
//!   notes land — adding them is an UPDATE, not a schema change).
//! - `dataset_main` the imported rows, one real SQL column per spreadsheet
//!   header. Only created when there is a spreadsheet.
//!
//! ponytail: no `__file_links`, no `metadata` blob, single dataset table —
//! add each when its consumer (media viewer, validation rules, multi-import)
//! exists. Versioned via `PRAGMA user_version` for future migrations.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};

/// `PRAGMA application_id` value so `file`-style tools can recognize `.qrate`.
const QRATE_APPLICATION_ID: i32 = 1097887558;
const QRATE_SCHEMA_VERSION: i32 = 1;

/// The `.qrate` file's name inside a project folder.
pub const PROJECT_FILE_NAME: &str = "project.qrate";

pub struct ProjectColumn {
    pub name: String,
    pub data_type: String,
    pub notes: String,
}

/// Everything read back from a `.qrate` file when a project is opened.
pub struct ProjectData {
    pub name: String,
    pub columns: Vec<ProjectColumn>,
    /// `dataset_main` column names, in table order (no `_row_id`).
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The currently open project — set when the launcher/wizard opens one, read
/// by the table (data) and workspace (per-project dock layout).
pub struct CurrentProject {
    /// The project folder (what recents show).
    pub dir: PathBuf,
    /// The `.qrate` file inside it.
    pub file: PathBuf,
    pub data: ProjectData,
}

impl gpui::Global for CurrentProject {}

/// Creates `path` (a `.qrate` file) with the v1 schema and the imported data.
/// `headers`/`rows` are the raw spreadsheet; `columns` the configured column
/// list (may differ from `headers` when a column config was loaded). Blank
/// projects pass empty `headers` and get no `dataset_main` table.
pub fn create_project_file(
    path: &Path,
    name: &str,
    source: &str,
    link_method: Option<&str>,
    columns: &[ProjectColumn],
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<()> {
    let conn = Connection::open(path).with_context(|| format!("Create project at {path:?}"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("Set WAL mode")?;
    conn.pragma_update(None, "application_id", QRATE_APPLICATION_ID)
        .context("Set application_id")?;
    conn.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
        .context("Set user_version")?;

    conn.execute_batch(
        r#"
        CREATE TABLE __settings (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        CREATE TABLE __columns (
          name      TEXT PRIMARY KEY,
          data_type TEXT NOT NULL,
          notes     TEXT
        );
        "#,
    )
    .context("Create schema")?;

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut settings: Vec<(&str, String)> = vec![
        ("name", name.to_string()),
        ("source", source.to_string()),
        ("created_at", created_at.to_string()),
    ];
    if let Some(m) = link_method {
        settings.push(("link_method", m.to_string()));
    }
    for (key, value) in &settings {
        conn.execute(
            "INSERT INTO __settings(key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .context("Insert setting")?;
    }

    for col in columns {
        conn.execute(
            "INSERT OR IGNORE INTO __columns(name, data_type, notes) VALUES (?1, ?2, ?3)",
            params![
                col.name,
                col.data_type,
                (!col.notes.is_empty()).then_some(&col.notes)
            ],
        )
        .context("Insert column")?;
    }

    if !headers.is_empty() {
        write_dataset(&conn, headers, rows)?;
    }

    // TRUNCATE the WAL before close so the project is a single file on disk
    // even if something else briefly holds a handle (see module docs).
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .context("Checkpoint WAL")?;
    Ok(())
}

/// Reads a whole `.qrate` file back: settings name, configured columns, and
/// the `dataset_main` contents (empty for blank projects).
pub fn load_project_file(path: &Path) -> Result<ProjectData> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Open project at {path:?}"))?;

    let name: String = conn
        .query_row("SELECT value FROM __settings WHERE key = 'name'", [], |r| {
            r.get(0)
        })
        .optional()
        .context("Read project name")?
        .unwrap_or_default();

    let mut columns = Vec::new();
    let mut stmt = conn.prepare("SELECT name, data_type, notes FROM __columns")?;
    let iter = stmt.query_map([], |r| {
        Ok(ProjectColumn {
            name: r.get(0)?,
            data_type: r.get(1)?,
            notes: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        })
    })?;
    for col in iter {
        columns.push(col.context("Read column")?);
    }
    drop(stmt);

    let (headers, rows) = load_dataset(&conn)?;
    Ok(ProjectData {
        name,
        columns,
        headers,
        rows,
    })
}

/// Reads `dataset_main` (headers from the table's own columns, then all rows).
/// A project without one (blank) yields empty vecs.
fn load_dataset(conn: &Connection) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'dataset_main'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut stmt = conn.prepare("SELECT * FROM dataset_main ORDER BY _row_id")?;
    // Column 0 is `_row_id`; the rest are the data columns, in creation order.
    let headers: Vec<String> = stmt
        .column_names()
        .into_iter()
        .skip(1)
        .map(|s| s.to_string())
        .collect();
    let n = headers.len();
    let mut rows = Vec::new();
    let mut query = stmt.query([])?;
    while let Some(row) = query.next()? {
        let mut cells = Vec::with_capacity(n);
        for i in 0..n {
            // Cells are written as TEXT, but be tolerant of NULLs.
            cells.push(row.get::<_, Option<String>>(i + 1)?.unwrap_or_default());
        }
        rows.push(cells);
    }
    Ok((headers, rows))
}

/// Reads one `__settings` value from a `.qrate` file (e.g. the dock layout).
pub fn read_setting(path: &Path, key: &str) -> Result<Option<String>> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Open project at {path:?}"))?;
    conn.query_row(
        "SELECT value FROM __settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
    .context("Read setting")
}

/// Upserts one `__settings` value. Opens a short-lived connection per call —
/// cheap for the current callers (dock-layout saves).
/// ponytail: sync write per call; add a debounced writer thread (like
/// `db::SettingsWriter`) if layout drags ever stutter.
pub fn write_setting(path: &Path, key: &str, value: &str) -> Result<()> {
    let conn = Connection::open(path).with_context(|| format!("Open project at {path:?}"))?;
    conn.execute(
        "INSERT INTO __settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .context("Upsert setting")?;
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .context("Checkpoint WAL")?;
    Ok(())
}

/// Creates `dataset_main` with one TEXT column per header and bulk-inserts the
/// rows. Ragged rows (flexible CSV) are padded/truncated to the header count.
fn write_dataset(conn: &Connection, headers: &[String], rows: &[Vec<String>]) -> Result<()> {
    let idents = dataset_column_idents(headers);
    let cols_sql: Vec<String> = idents.iter().map(|i| format!("{i} TEXT")).collect();
    conn.execute_batch(&format!(
        "CREATE TABLE dataset_main (_row_id INTEGER PRIMARY KEY, {});",
        cols_sql.join(", ")
    ))
    .context("Create dataset_main")?;

    let placeholders: Vec<String> = (1..=idents.len()).map(|i| format!("?{i}")).collect();
    let insert_sql = format!(
        "INSERT INTO dataset_main ({}) VALUES ({})",
        idents.join(", "),
        placeholders.join(", ")
    );
    conn.execute_batch("BEGIN")?;
    {
        let mut stmt = conn.prepare(&insert_sql).context("Prepare row insert")?;
        let empty = String::new();
        for row in rows {
            let padded: Vec<&String> = (0..idents.len())
                .map(|i| row.get(i).unwrap_or(&empty))
                .collect();
            stmt.execute(rusqlite::params_from_iter(padded))
                .context("Insert row")?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Quotes each header as a SQL identifier, de-duplicating case-insensitively
/// (`Title`, `title` → `"Title"`, `"title_2"`) and naming blanks `column_N` —
/// spreadsheet headers are user data and can collide or be empty.
fn dataset_column_idents(headers: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    headers
        .iter()
        .enumerate()
        .map(|(ix, h)| {
            let base = h.trim();
            let base = if base.is_empty() {
                format!("column_{}", ix + 1)
            } else {
                base.to_string()
            };
            let mut name = base.clone();
            let mut n = 2;
            while !seen.insert(name.to_lowercase()) {
                name = format!("{base}_{n}");
                n += 1;
            }
            format!("\"{}\"", name.replace('"', "\"\""))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempfile(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("qrate-project-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn round_trips_a_project() {
        let path = tempfile("roundtrip.qrate");
        let headers = vec!["Digital ID".to_string(), "Title".to_string()];
        let rows = vec![
            vec!["1".to_string(), "First".to_string()],
            vec!["2".to_string()], // ragged: short row is padded
        ];
        let columns = vec![ProjectColumn {
            name: "Title".into(),
            data_type: "Text".into(),
            notes: String::new(),
        }];
        create_project_file(
            &path,
            "Test Project",
            "CSV + folder",
            Some("exact filename"),
            &columns,
            &headers,
            &rows,
        )
        .unwrap();

        // Single file on disk — WAL siblings gone after close.
        assert!(path.exists());
        assert!(!path.with_extension("qrate-wal").exists());

        let conn = Connection::open(&path).unwrap();
        let app_id: i32 = conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .unwrap();
        assert_eq!(app_id, QRATE_APPLICATION_ID);
        let name: String = conn
            .query_row("SELECT value FROM __settings WHERE key = 'name'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Test Project");
        let (count, padded): (i64, String) = conn
            .query_row(
                "SELECT count(*), (SELECT \"Title\" FROM dataset_main WHERE \"Digital ID\" = '2') FROM dataset_main",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(padded, "");
        let col_count: i64 = conn
            .query_row("SELECT count(*) FROM __columns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(col_count, 1);
        drop(conn);

        // Full read-back path (what opening a project uses).
        let data = load_project_file(&path).unwrap();
        assert_eq!(data.name, "Test Project");
        assert_eq!(data.headers, vec!["Digital ID", "Title"]);
        assert_eq!(data.rows.len(), 2);
        assert_eq!(data.rows[0], vec!["1", "First"]);
        assert_eq!(data.rows[1], vec!["2", ""]);
        assert_eq!(data.columns.len(), 1);

        // Per-key settings round trip (dock layout persistence).
        assert_eq!(read_setting(&path, "dock_layout").unwrap(), None);
        write_setting(&path, "dock_layout", "{}").unwrap();
        write_setting(&path, "dock_layout", "{\"v\":2}").unwrap();
        assert_eq!(
            read_setting(&path, "dock_layout").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
        // Read-only handles can leave an empty `-wal` behind (they may not
        // delete it on close) — the invariant is no *unflushed* data in it.
        let wal_len = std::fs::metadata(path.with_extension("qrate-wal"))
            .map(|m| m.len())
            .unwrap_or(0);
        assert_eq!(wal_len, 0);
    }

    #[test]
    fn blank_project_has_no_dataset_table() {
        let path = tempfile("blank.qrate");
        create_project_file(&path, "Blank", "Blank", None, &[], &[], &[]).unwrap();
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'dataset_main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn dedupes_and_quotes_headers() {
        let idents = dataset_column_idents(&[
            "Title".to_string(),
            "title".to_string(),
            "".to_string(),
            "Weird\"Name".to_string(),
        ]);
        assert_eq!(
            idents,
            vec![
                "\"Title\"",
                "\"title_2\"",
                "\"column_3\"",
                "\"Weird\"\"Name\"",
            ]
        );
    }
}
