//! On-disk `.qrate` project file (v2). One SQLite database per project,
//! written once by the "New Project" wizard. Runs in `journal_mode=DELETE`,
//! not WAL: with a single short-lived connection per operation WAL buys no
//! concurrency, and its `-wal`/`-shm` siblings only vanish when the *last*
//! connection closes cleanly — the exact lifecycle bug ASNT-16's probe
//! characterized. In DELETE mode a `-journal` exists only mid-transaction, so
//! a clean commit always leaves a single file on disk, and a crash leaves a
//! hot journal that SQLite auto-recovers on the next open.
//!
//! v2 schema — only what has a consumer today:
//! - `__settings`  key/value: project name, source kind, link method, created_at.
//! - `__columns`   configured columns (`notes` is where ASNT-18's per-column
//!   notes land — adding them is an UPDATE, not a schema change).
//! - `__notes`     authored diagnostics: imported cell notes today, user marks
//!   later. Its `dataset` column is why a second sheet will be new rows rather
//!   than a new table. Computed diagnostics (validators, spell-check) are never
//!   stored — they are recomputed on open, so persisting them only goes stale.
//! - `dataset_main` the imported rows, one real SQL column per spreadsheet
//!   header. Only created when there is a spreadsheet.
//!
//! ponytail: no `__file_links`, no `metadata` blob, single dataset table —
//! add each when its consumer (media viewer, validation rules, multi-import)
//! exists. Versioned via `PRAGMA user_version` for future migrations; v1 files
//! get `__notes` created lazily on first write rather than a migration pass, so
//! opening a project stays a read-only operation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};

/// `PRAGMA application_id` value so `file`-style tools can recognize `.qrate`.
const QRATE_APPLICATION_ID: i32 = 1097887558;
const QRATE_SCHEMA_VERSION: i32 = 2;

/// `__settings` key for the files folder chosen in the wizard's Files step. Only the path is
/// kept — qrate never copies source files, so the table crate re-resolves row images against
/// this folder every time the project opens (see `table::photos`).
pub const FILES_FOLDER_KEY: &str = "files_folder";

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
    /// Every `__settings` row, cached so project-scope setting reads (e.g. a live table
    /// repaint) don't hit the DB. Writers keep it current via `CurrentProject::set_bool`.
    pub values: HashMap<String, crate::Val>,
}

/// The currently open project — set when the launcher/wizard opens one, read
/// by the table (data) and workspace (per-project dock layout).
pub struct CurrentProject {
    /// The `.qrate` file (the project *is* this one file — no wrapper folder).
    pub file: PathBuf,
    pub data: ProjectData,
}

impl gpui::Global for CurrentProject {}

impl CurrentProject {
    /// Resolved display name for titles/UI: the stored project name, else the
    /// file stem, else "Untitled Project". The one place this fallback lives.
    pub fn display_name(&self) -> String {
        if !self.data.name.is_empty() {
            return self.data.name.clone();
        }
        self.file
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled Project".into())
    }

    /// Project-scoped boolean setting, `false` if unset.
    pub fn get_bool(&self, key: &str) -> bool {
        self.data.values.get(key).map(|v| v.bool()).unwrap_or(false)
    }

    /// Sets a project-scoped boolean: updates the in-memory cache and queues a debounced
    /// write to the `.qrate` file. Mutating the global fires observers so readers repaint.
    pub fn set_bool(key: &'static str, val: bool, cx: &mut gpui::App) {
        let file = {
            let p = cx.global_mut::<Self>();
            p.data.values.insert(key.into(), crate::Val::Bool(val));
            p.file.clone()
        };
        queue_write(&file, key, if val { "true" } else { "false" }, cx);
    }

    /// Sets a project-scoped text value. See [`set_bool`](Self::set_bool).
    pub fn set_text(key: &'static str, val: gpui::SharedString, cx: &mut gpui::App) {
        let (file, value) = {
            let p = cx.global_mut::<Self>();
            p.data
                .values
                .insert(key.into(), crate::Val::Text(val.clone()));
            (p.file.clone(), val)
        };
        queue_write(&file, key, &value, cx);
    }

    /// Declares a column's type, updating the cache validators read and writing `__columns`.
    /// Synchronous rather than queued: [`ProjectSettingsWriter`] is keyed by `__settings` key and
    /// this is another table, and picking a type is one deliberate click, not a drag.
    pub fn set_column_type(name: &str, data_type: &str, cx: &mut gpui::App) {
        let file = {
            let p = cx.global_mut::<Self>();
            match p.data.columns.iter_mut().find(|c| c.name == name) {
                Some(column) => column.data_type = data_type.to_string(),
                None => p.data.columns.push(ProjectColumn {
                    name: name.to_string(),
                    data_type: data_type.to_string(),
                    notes: String::new(),
                }),
            }
            p.file.clone()
        };
        if let Err(err) = write_column_type(&file, name, data_type) {
            log::error!("failed to save the type of column {name}: {err}");
        }
        crate::dirty::mark(crate::dirty::COLUMN_SETTINGS, cx);
    }
}

/// Opens a `.qrate` read-write with the pragmas every connection wants:
/// DELETE journaling (also converts any WAL-era file back, which removes its
/// stale `-wal`/`-shm` siblings), FULL synchronous — the durable setting for
/// rollback journals; NORMAL is only equivalent under WAL — and a busy
/// timeout so a transient AV/indexer file lock retries instead of surfacing
/// as SQLITE_BUSY.
fn open_rw(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("Open project at {path:?}"))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("Set busy timeout")?;
    conn.pragma_update(None, "journal_mode", "DELETE")
        .context("Set journal mode")?;
    conn.pragma_update(None, "synchronous", "FULL")
        .context("Set synchronous")?;
    Ok(conn)
}

/// Read-only open; no journal-mode change (that needs a write handle).
fn open_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Open project at {path:?}"))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("Set busy timeout")?;
    Ok(conn)
}

/// Creates `path` (a `.qrate` file) with the v2 schema and the imported data.
/// `headers`/`rows` are the raw spreadsheet; `columns` the configured column
/// list (may differ from `headers` when a column config was loaded). Blank
/// projects pass empty `headers` and get no `dataset_main` table. `files_folder`
/// is the wizard's Files-step folder path, if one was chosen and linked.
#[allow(clippy::too_many_arguments)]
pub fn create_project_file(
    path: &Path,
    name: &str,
    source: &str,
    link_method: Option<&str>,
    files_folder: Option<&str>,
    columns: &[ProjectColumn],
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<()> {
    let conn = open_rw(path).with_context(|| format!("Create project at {path:?}"))?;
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
    conn.execute_batch(NOTES_DDL).context("Create __notes")?;

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
    if let Some(folder) = files_folder.filter(|f| !f.trim().is_empty()) {
        settings.push((FILES_FOLDER_KEY, folder.to_string()));
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
    Ok(())
}

/// Reads a whole `.qrate` file back: settings name, configured columns, and
/// the `dataset_main` contents (empty for blank projects).
pub fn load_project_file(path: &Path) -> Result<ProjectData> {
    let conn = open_ro(path)?;

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

    let mut values = HashMap::new();
    let mut stmt = conn.prepare("SELECT key, value FROM __settings")?;
    let iter = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in iter {
        let (key, value) = row.context("Read setting")?;
        values.insert(key, crate::Val::Text(value.into()));
    }
    drop(stmt);

    let (headers, rows) = load_dataset(&conn)?;
    Ok(ProjectData {
        name,
        columns,
        headers,
        rows,
        values,
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
    let conn = open_ro(path)?;
    conn.query_row(
        "SELECT value FROM __settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
    .context("Read setting")
}

/// Upserts one `__settings` value, synchronously. Hot-path callers (dock
/// layout, window bounds — per drag event) should go through
/// [`queue_write`] instead so the UI thread never blocks on file I/O.
pub fn write_setting(path: &Path, key: &str, value: &str) -> Result<()> {
    let conn = open_rw(path)?;
    conn.execute(
        "INSERT INTO __settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .context("Upsert setting")?;
    Ok(())
}

/// Upserts one column's declared type. A column the project was created without — a spreadsheet
/// header nobody configured — gets a row, which is what lets a type be set on any column the table
/// shows rather than only the ones the wizard wrote.
pub fn write_column_type(path: &Path, name: &str, data_type: &str) -> Result<()> {
    let conn = open_rw(path)?;
    conn.execute(
        "INSERT INTO __columns(name, data_type, notes) VALUES (?1, ?2, NULL)
         ON CONFLICT(name) DO UPDATE SET data_type = excluded.data_type",
        params![name, data_type],
    )
    .context("Upsert column type")?;
    Ok(())
}

/// Re-key a column's declared type and description to its new name. `__columns` is keyed by name,
/// so without this a rename would look like the old column vanishing and a fresh, untyped one
/// appearing. No row for that name (a column nobody configured) is nothing to move.
pub fn rename_column(path: &Path, before: &str, after: &str) -> Result<()> {
    let conn = open_rw(path)?;
    conn.execute(
        "UPDATE __columns SET name = ?2 WHERE name = ?1",
        params![before, after],
    )
    .context("Rename column")?;
    Ok(())
}

/// Shared by project creation and [`write_notes`], which creates the table on demand so a v1
/// file gains it on first write instead of needing a migration pass on open.
const NOTES_DDL: &str = r#"
    CREATE TABLE IF NOT EXISTS __notes (
      dataset     TEXT NOT NULL,
      row_ix      INTEGER,
      column_name TEXT,
      severity    TEXT NOT NULL,
      source      TEXT NOT NULL,
      message     TEXT NOT NULL,
      created_at  TEXT,
      author      TEXT
    );
"#;

/// Bring a `__notes` table written before notes carried provenance up to the columns above.
/// Nullable, so every existing row reads back with no date and no author and renders as the bare
/// note it has always been — there is nothing to backfill, because that information was never
/// captured. Idempotent: a duplicate-column error is the table already being current.
fn add_note_provenance(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    for column in ["created_at", "author"] {
        match tx.execute(&format!("ALTER TABLE __notes ADD COLUMN {column} TEXT"), []) {
            Ok(_) => {}
            // `duplicate column name` is the only error worth swallowing; anything else means the
            // table is not what we think it is and the caller should hear about it.
            Err(err) if err.to_string().contains("duplicate column") => {}
            Err(err) => return Err(err).context("Add note provenance"),
        }
    }
    Ok(())
}

/// One `__notes` row. Deliberately flat and stringly-typed rather than reusing
/// `diagnostics::Diagnostic`: that crate depends on this one, so the schema stays owned by the
/// module that documents it. A `None` coordinate widens the note — row-only marks a whole row,
/// column-only a whole column, neither the dataset itself. The `source` column stays out of the
/// struct: [`write_notes`] owns it for the whole batch, so a note can't carry a mismatching one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredNote {
    pub dataset: String,
    pub row: Option<usize>,
    pub column: Option<String>,
    pub severity: String,
    pub message: String,
    /// When the note was filed and by whom, as free text. `None` on every note written before the
    /// columns existed, and on any note filed with no author configured — a catalogue's marginalia
    /// is worth keeping even unsigned.
    pub created_at: Option<String>,
    pub author: Option<String>,
}

/// Today, from SQLite's own clock — the one dependency here that already knows what day it is.
/// Local rather than UTC: an archivist reading "filed 2026-08-14" means their own Tuesday.
pub fn today(path: &Path) -> Option<String> {
    open_ro(path)
        .ok()?
        .query_row("SELECT date('now','localtime')", [], |r| r.get(0))
        .ok()
}

/// Every stored note. A file written before `__notes` existed yields an empty vec, the same
/// tolerance [`load_dataset`] gives a blank project's missing `dataset_main`.
pub fn read_notes(path: &Path) -> Result<Vec<StoredNote>> {
    let conn = open_ro(path)?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '__notes'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }

    // A file written before notes carried provenance has neither column, and `write_notes` only
    // adds them when it next saves. Named in the projection so the read works either way.
    let provenance: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('__notes') WHERE name = 'created_at'",
        [],
        |r| r.get(0),
    )?;
    let columns = match provenance {
        0 => "NULL AS created_at, NULL AS author",
        _ => "created_at, author",
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT dataset, row_ix, column_name, severity, message, {columns} FROM __notes"
    ))?;
    let iter = stmt.query_map([], |r| {
        Ok(StoredNote {
            dataset: r.get(0)?,
            row: r.get::<_, Option<i64>>(1)?.map(|v| v as usize),
            column: r.get(2)?,
            severity: r.get(3)?,
            message: r.get(4)?,
            created_at: r.get(5)?,
            author: r.get(6)?,
        })
    })?;
    iter.collect::<rusqlite::Result<Vec<_>>>()
        .context("Read notes")
}

/// Replaces everything `source` previously stored, in one transaction — the same
/// replace-by-source rule the in-memory store uses, so a re-import can't leave orphans behind.
/// Only authored sources belong here (imported notes, user marks); computed diagnostics are
/// recomputed on open and would go stale in storage.
pub fn write_notes(path: &Path, source: &str, notes: &[StoredNote]) -> Result<()> {
    let mut conn = open_rw(path)?;
    let tx = conn.transaction()?;
    tx.execute_batch(NOTES_DDL).context("Create __notes")?;
    add_note_provenance(&tx)?;
    tx.execute("DELETE FROM __notes WHERE source = ?1", params![source])
        .context("Clear notes")?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO __notes(dataset, row_ix, column_name, severity, source, message, created_at, author)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for n in notes {
            stmt.execute(params![
                n.dataset,
                n.row.map(|v| v as i64),
                n.column,
                n.severity,
                source,
                n.message,
                n.created_at,
                n.author
            ])
            .context("Insert note")?;
        }
    }
    tx.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
        .context("Set user_version")?;
    tx.commit().context("Commit notes")?;
    Ok(())
}

/// Debounced background writer for `__settings` values. The dock-layout and
/// window-bounds observers fire on every drag event; latest value per
/// (file, key) wins, and one thread serves all project files — no lifecycle
/// to manage on project switch, since the path travels with each entry.
/// Mirrors `db::SettingsWriter`.
#[derive(Clone)]
pub struct ProjectSettingsWriter {
    pending: Arc<Mutex<HashMap<(PathBuf, String), String>>>,
    wake: mpsc::Sender<()>,
}

impl ProjectSettingsWriter {
    pub fn start() -> Self {
        let pending: Arc<Mutex<HashMap<(PathBuf, String), String>>> = Arc::default();
        let (wake, rx) = mpsc::channel::<()>();
        let map = pending.clone();
        std::thread::spawn(move || {
            let debounce = Duration::from_millis(450);
            while rx.recv().is_ok() {
                // Something was enqueued — let the burst settle (drain wake-ups
                // until one debounce window passes quietly), then write.
                while rx.recv_timeout(debounce).is_ok() {}
                Self::write_pending(&map);
            }
            // Channel closed (writer dropped) — flush whatever is left.
            Self::write_pending(&map);
        });
        Self { pending, wake }
    }

    pub fn enqueue(&self, file: &Path, key: &str, value: String) {
        if let Ok(mut map) = self.pending.lock() {
            map.insert((file.to_path_buf(), key.to_string()), value);
        }
        let _ = self.wake.send(());
    }

    /// Writes everything still pending, synchronously — the app-quit path,
    /// which can't wait out the debounce window.
    pub fn flush(&self) {
        Self::write_pending(&self.pending);
    }

    fn write_pending(pending: &Mutex<HashMap<(PathBuf, String), String>>) {
        let drained: Vec<_> = match pending.lock() {
            Ok(mut map) => map.drain().collect(),
            Err(_) => return,
        };
        for ((file, key), value) in drained {
            if let Err(err) = write_setting(&file, &key, &value) {
                log::error!("failed to save project setting {key}: {err}");
            }
        }
    }
}

/// App-wide handle to the one [`ProjectSettingsWriter`], set at startup.
#[derive(Clone, Default)]
pub struct ProjectPersistence {
    pub writer: Option<ProjectSettingsWriter>,
}

impl gpui::Global for ProjectPersistence {}

/// Queues a debounced project-setting write; falls back to a synchronous
/// write when the writer global isn't set (tests, early startup).
pub fn queue_write(file: &Path, key: &str, value: &str, cx: &gpui::App) {
    let writer = cx
        .try_global::<ProjectPersistence>()
        .and_then(|p| p.writer.clone());
    match writer {
        Some(w) => w.enqueue(file, key, value.to_string()),
        None => {
            if let Err(err) = write_setting(file, key, value) {
                log::error!("failed to save project setting {key}: {err}");
            }
        }
    }
}

/// Creates `dataset_main` (one TEXT column per header) and bulk-inserts the rows. Ragged rows
/// (flexible CSV) are padded/truncated to the header count. Manages no transaction of its own —
/// the caller wraps it, so the create and the inserts commit together (and, on a re-save,
/// atomically with the preceding drop).
fn create_and_fill_dataset(
    conn: &Connection,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<()> {
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
    let mut stmt = conn.prepare(&insert_sql).context("Prepare row insert")?;
    let empty = String::new();
    for row in rows {
        let padded: Vec<&String> = (0..idents.len())
            .map(|i| row.get(i).unwrap_or(&empty))
            .collect();
        stmt.execute(rusqlite::params_from_iter(padded))
            .context("Insert row")?;
    }
    Ok(())
}

/// Creates and fills `dataset_main` in one transaction (the create-time path).
fn write_dataset(conn: &Connection, headers: &[String], rows: &[Vec<String>]) -> Result<()> {
    conn.execute_batch("BEGIN")?;
    create_and_fill_dataset(conn, headers, rows)?;
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Rewrites `dataset_main` from the in-memory rows — the whole table, in one transaction, so a
/// crash mid-save leaves the previous version intact (an uncommitted transaction rolls back when
/// the connection drops). `headers`/`rows` must be in the project's *original* column order: the
/// `.qrate` schema keeps a fixed physical column order, and the separately-saved column layout
/// maps that to display order. A blank project (no headers, no `dataset_main`) is a no-op.
pub fn save_dataset(path: &Path, headers: &[String], rows: &[Vec<String>]) -> Result<()> {
    if headers.is_empty() {
        return Ok(());
    }
    let conn = open_rw(path)?;
    conn.execute_batch("BEGIN; DROP TABLE IF EXISTS dataset_main;")
        .context("Begin dataset rewrite")?;
    create_and_fill_dataset(&conn, headers, rows)?;
    conn.execute_batch("COMMIT")
        .context("Commit dataset rewrite")?;
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
        // Also clear journal/WAL siblings a previous (possibly WAL-era) run left.
        for ext in ["qrate-journal", "qrate-wal", "qrate-shm"] {
            let _ = std::fs::remove_file(path.with_extension(ext));
        }
        path
    }

    fn note(row: Option<usize>, column: Option<&str>, msg: &str) -> StoredNote {
        StoredNote {
            dataset: "dataset_main".into(),
            row,
            column: column.map(str::to_string),
            severity: "note".into(),
            message: msg.into(),
            created_at: None,
            author: None,
        }
    }

    fn blank_project(path: &Path) {
        create_project_file(path, "Notes", "CSV", None, None, &[], &[], &[]).unwrap();
    }

    /// A project written before notes carried a date and an author must still open, still read its
    /// notes, and gain the columns on the next save — losing a catalogue's marginalia to a schema
    /// change is the one outcome here that cannot be undone.
    #[test]
    fn a_project_predating_note_provenance_reads_and_upgrades() {
        let path = tempfile("notes-legacy.qrate");
        blank_project(&path);

        // The pre-provenance table, verbatim.
        {
            let conn = open_rw(&path).unwrap();
            conn.execute_batch(
                r#"
                DROP TABLE IF EXISTS __notes;
                CREATE TABLE __notes (
                  dataset     TEXT NOT NULL,
                  row_ix      INTEGER,
                  column_name TEXT,
                  severity    TEXT NOT NULL,
                  source      TEXT NOT NULL,
                  message     TEXT NOT NULL
                );
                INSERT INTO __notes VALUES
                  ('dataset_main', 2, 'Title', 'note', 'import', 'verso inscription');
                "#,
            )
            .unwrap();
        }

        let old = read_notes(&path).unwrap();
        assert_eq!(old.len(), 1, "the old note survives the schema it predates");
        assert_eq!(old[0].message, "verso inscription");
        assert_eq!(old[0].created_at, None, "nothing to backfill, so no date");

        // Saving anything upgrades the table, and a note filed now carries its provenance.
        let mut fresh = note(Some(2), Some("Title"), "identified by her daughter");
        fresh.created_at = Some("2026-08-14".into());
        fresh.author = Some("rk".into());
        write_notes(&path, "import", &[fresh]).unwrap();

        let after = read_notes(&path).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].created_at.as_deref(), Some("2026-08-14"));
        assert_eq!(after[0].author.as_deref(), Some("rk"));
    }

    #[test]
    fn notes_round_trip_and_replace_by_source() {
        let path = tempfile("notes.qrate");
        blank_project(&path);

        write_notes(
            &path,
            "import",
            &[
                note(Some(0), Some("Title"), "cell note"),
                note(Some(3), None, "whole row"),
            ],
        )
        .unwrap();
        assert_eq!(read_notes(&path).unwrap().len(), 2);

        // A different source is additive — it must not disturb `import`'s entries.
        write_notes(&path, "spell", &[note(None, Some("Title"), "whole column")]).unwrap();
        assert_eq!(read_notes(&path).unwrap().len(), 3);

        // Optional coordinates survive the NULL round-trip in both directions.
        let all = read_notes(&path).unwrap();
        let row_only = all.iter().find(|n| n.message == "whole row").unwrap();
        assert_eq!((row_only.row, row_only.column.as_deref()), (Some(3), None));
        let col_only = all.iter().find(|n| n.message == "whole column").unwrap();
        assert_eq!(
            (col_only.row, col_only.column.as_deref()),
            (None, Some("Title"))
        );

        // Republishing an empty set clears only that source — the invalidation rule.
        write_notes(&path, "import", &[]).unwrap();
        let left = read_notes(&path).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].message, "whole column");

        assert!(!path.with_extension("qrate-journal").exists());
    }

    #[test]
    fn reads_notes_from_a_file_predating_the_table() {
        let path = tempfile("v1.qrate");
        blank_project(&path);
        Connection::open(&path)
            .unwrap()
            .execute_batch("DROP TABLE __notes")
            .unwrap();

        // A v1 `.qrate` is missing the table entirely; that is empty, not an error.
        assert_eq!(read_notes(&path).unwrap(), Vec::new());
        // ...and the first write creates it, which is the whole 1 -> 2 migration.
        write_notes(&path, "import", &[note(Some(1), Some("A"), "hi")]).unwrap();
        assert_eq!(read_notes(&path).unwrap().len(), 1);
    }

    /// The wizard writes `__columns` once and nothing has updated it since, so both halves matter:
    /// retyping a configured column, and typing one the wizard never wrote a row for.
    #[test]
    fn a_column_type_is_upserted_and_survives_a_reload() {
        let path = tempfile("types.qrate");
        create_project_file(
            &path,
            "Types",
            "CSV + folder",
            None,
            None,
            &[ProjectColumn {
                name: "Digital ID".into(),
                data_type: "Text".into(),
                notes: "the scan".into(),
            }],
            &["Digital ID".to_string()],
            &[vec!["1".to_string()]],
        )
        .unwrap();

        write_column_type(&path, "Digital ID", "Filename").unwrap();
        write_column_type(&path, "Taken", "Date").unwrap();

        let columns = load_project_file(&path).unwrap().columns;
        let by_name = |n: &str| {
            columns
                .iter()
                .find(|c| c.name == n)
                .map(|c| (c.data_type.as_str(), c.notes.as_str()))
        };
        assert_eq!(by_name("Digital ID"), Some(("Filename", "the scan")));
        assert_eq!(by_name("Taken"), Some(("Date", "")));
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
            Some("/photos"),
            &columns,
            &headers,
            &rows,
        )
        .unwrap();

        // Single file on disk — DELETE mode removes the `-journal` at commit,
        // and no `-wal`/`-shm` are ever created.
        assert!(path.exists());
        assert!(!path.with_extension("qrate-journal").exists());
        assert!(!path.with_extension("qrate-wal").exists());
        assert!(!path.with_extension("qrate-shm").exists());

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
        assert_eq!(
            data.values.get(FILES_FOLDER_KEY).map(|v| v.text()),
            Some("/photos".into())
        );

        // Per-key settings round trip (dock layout persistence).
        assert_eq!(read_setting(&path, "dock_layout").unwrap(), None);
        write_setting(&path, "dock_layout", "{}").unwrap();
        write_setting(&path, "dock_layout", "{\"v\":2}").unwrap();
        assert_eq!(
            read_setting(&path, "dock_layout").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
        // DELETE mode's invariant: nothing but the `.qrate` file at rest.
        assert!(!path.with_extension("qrate-journal").exists());
        assert!(!path.with_extension("qrate-wal").exists());
    }

    #[test]
    fn save_dataset_rewrites_rows_and_round_trips() {
        let path = tempfile("save.qrate");
        let headers = vec!["Digital ID".to_string(), "Title".to_string()];
        let rows = vec![vec!["1".to_string(), "First".to_string()]];
        create_project_file(&path, "S", "CSV", None, None, &[], &headers, &rows).unwrap();

        // Edit a cell and add a row, then save the whole table back.
        let edited = vec![
            vec!["1".to_string(), "Edited".to_string()],
            vec!["2".to_string(), "Second".to_string()],
        ];
        save_dataset(&path, &headers, &edited).unwrap();

        let data = load_project_file(&path).unwrap();
        assert_eq!(data.headers, headers);
        assert_eq!(data.rows, edited);
        // DELETE mode's invariant survives the rewrite: only the `.qrate` file at rest.
        assert!(!path.with_extension("qrate-journal").exists());
        assert!(!path.with_extension("qrate-wal").exists());
    }

    #[test]
    fn load_project_file_caches_settings_values() {
        let path = tempfile("values.qrate");
        create_project_file(&path, "V", "Blank", None, None, &[], &[], &[]).unwrap();
        write_setting(&path, "table_stripes", "true").unwrap();

        let data = load_project_file(&path).unwrap();
        assert_eq!(data.name, "V");
        assert!(data.values.get("table_stripes").unwrap().bool());
        // Creation-time settings are cached too.
        assert_eq!(data.values.get("source").unwrap().text(), "Blank");
        assert!(!data.values.contains_key("table_stripes_missing"));
    }

    #[test]
    fn writer_flushes_latest_value_per_key() {
        let path = tempfile("writer.qrate");
        create_project_file(&path, "W", "Blank", None, None, &[], &[], &[]).unwrap();

        let writer = ProjectSettingsWriter::start();
        writer.enqueue(&path, "dock_layout", "{\"v\":1}".into());
        writer.enqueue(&path, "dock_layout", "{\"v\":2}".into()); // latest wins
        writer.flush(); // quit path: synchronous, doesn't wait out the debounce
        assert_eq!(
            read_setting(&path, "dock_layout").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
    }

    #[test]
    fn blank_project_has_no_dataset_table() {
        let path = tempfile("blank.qrate");
        create_project_file(&path, "Blank", "Blank", None, None, &[], &[], &[]).unwrap();
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
