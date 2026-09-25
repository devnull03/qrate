//! On-disk `.qrate` project file (v4). One SQLite database per project,
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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

const COLUMN_NOTES_WRITE_PREFIX: &str = "column_notes:";
const COLUMN_TYPE_WRITE_PREFIX: &str = "column_type:";

use anyhow::{Context as _, Result};
use qrate_export::{QRATE_APPLICATION_ID, QRATE_SCHEMA_VERSION, table_exists};
pub use qrate_export::{RowStructure, SourceKind};
use rusqlite::{Connection, OptionalExtension as _, params};

/// Private identity of one `dataset_main` row. Unlike the source index shown in the `#` column,
/// this value survives inserts, deletes, and save/reload cycles.
pub type RowId = i64;

const ROW_STRUCTURE_DDL: &str = r#"
    CREATE TABLE IF NOT EXISTS __row_structure (
      row_id        INTEGER PRIMARY KEY,
      parent_id     INTEGER REFERENCES dataset_main(_row_id) ON DELETE CASCADE,
      level_key     TEXT NOT NULL,
      sibling_order INTEGER NOT NULL,
      source_path   TEXT,
      source_kind   TEXT CHECK (source_kind IN ('file', 'directory')),
      FOREIGN KEY(row_id) REFERENCES dataset_main(_row_id) ON DELETE CASCADE
    );
"#;

/// `__settings` key for the files folder chosen in the wizard's Files step. Only the path is
/// kept — qrate never copies source files, so the table crate re-resolves row images against
/// this folder every time the project opens (see `table::photos`).
pub const FILES_FOLDER_KEY: &str = "files_folder";

/// What an import does with material the project already holds: skip, update, or add_as_new.
pub const IMPORT_DUPLICATE_POLICY_KEY: &str = "import_duplicate_policy";

/// `__settings` key for the spreadsheet this project pushes to. The id alone, not the URL — the
/// link is derivable (`data_exchange::google::sheet_url`) and the id is what the Sheets API takes.
/// Written when the user picks a sheet through Google's chooser, which is also what grants qrate
/// access to it; storing an id the user typed would name a file the token cannot reach.
pub const GOOGLE_SHEET_ID_KEY: &str = "google_sheet_id";

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
    /// Stable identities parallel to `rows`; never exposed as a table column.
    pub row_ids: Vec<RowId>,
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

    /// Drops this project's own value for `key`, so reads fall back to the user-wide default.
    /// Queued like a write to the same key, so whichever came last wins.
    pub fn clear(key: &str, cx: &mut gpui::App) {
        let file = {
            let p = cx.global_mut::<Self>();
            p.data.values.remove(key);
            p.file.clone()
        };
        queue(&file, key, None, cx);
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

    /// Follow the grid's column set. The Settings pages, the declared types and the preview
    /// resolver all read this list, so a column added or renamed in the grid stays invisible to
    /// them until it lands here.
    pub fn set_columns(headers: Vec<String>, renamed: Option<(&str, &str)>, cx: &mut gpui::App) {
        // Undo lands here after every step, and a write wakes every observer of the project.
        if cx
            .try_global::<Self>()
            .is_none_or(|project| renamed.is_none() && project.data.headers == headers)
        {
            return;
        }
        let project = cx.global_mut::<Self>();
        if let Some((before, after)) = renamed
            && let Some(column) = project.data.columns.iter_mut().find(|c| c.name == before)
        {
            column.name = after.to_string();
        }
        project.data.headers = headers;
    }
    /// Declares a column's type, updating the cache validators read and queueing the `__columns`
    /// write — the column that held a single-holder type before gives it up in the same batch.
    pub fn set_column_type(name: &str, data_type: &str, cx: &mut gpui::App) {
        let kind = crate::columns::ColumnType::from_declared(data_type);
        let single_holder = matches!(
            kind,
            crate::columns::ColumnType::Title | crate::columns::ColumnType::Filename
        );
        let (file, cleared) = {
            let p = cx.global_mut::<Self>();
            let mut cleared = Vec::new();
            if single_holder {
                for column in &mut p.data.columns {
                    if column.name != name
                        && crate::columns::ColumnType::from_declared(&column.data_type) == kind
                    {
                        column.data_type = crate::columns::ColumnType::Text.as_str().to_string();
                        cleared.push(column.name.clone());
                    }
                }
            }
            match p.data.columns.iter_mut().find(|c| c.name == name) {
                Some(column) => column.data_type = data_type.to_string(),
                None => p.data.columns.push(ProjectColumn {
                    name: name.to_string(),
                    data_type: data_type.to_string(),
                    notes: String::new(),
                }),
            }
            (p.file.clone(), cleared)
        };
        for cleared in cleared {
            let key = format!("{COLUMN_TYPE_WRITE_PREFIX}{cleared}");
            let text = crate::columns::ColumnType::Text.as_str().to_string();
            queue(&file, &key, Some(text), cx);
        }
        let key = format!("{COLUMN_TYPE_WRITE_PREFIX}{name}");
        queue(&file, &key, Some(data_type.to_string()), cx);
        crate::dirty::mark(crate::dirty::COLUMN_SETTINGS, cx);
    }

    /// Updates a column description in memory immediately and queues the durable `__columns`
    /// write, so typing does not force SQLite to fsync each keystroke.
    pub fn set_column_notes(name: &str, notes: &str, cx: &mut gpui::App) {
        let file = {
            let project = cx.global_mut::<Self>();
            match project
                .data
                .columns
                .iter_mut()
                .find(|column| column.name == name)
            {
                Some(column) => column.notes = notes.to_string(),
                None => project.data.columns.push(ProjectColumn {
                    name: name.to_string(),
                    data_type: crate::columns::ColumnType::Text.as_str().to_string(),
                    notes: notes.to_string(),
                }),
            }
            project.file.clone()
        };
        let key = format!("{COLUMN_NOTES_WRITE_PREFIX}{name}");
        queue_write(&file, &key, notes, cx);
        crate::dirty::mark(crate::dirty::COLUMN_SETTINGS, cx);
    }
}

/// Opens a `.qrate` read-write with the pragmas every connection wants:
/// DELETE journaling (also converts any WAL-era file back, which removes its
/// stale `-wal`/`-shm` siblings), FULL synchronous — the durable setting for
/// rollback journals; NORMAL is only equivalent under WAL — and a busy
/// timeout so a transient AV/indexer file lock retries instead of surfacing
/// as SQLITE_BUSY.
pub(crate) fn open_rw(path: &Path) -> Result<Connection> {
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
pub(crate) fn open_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Open project at {path:?}"))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("Set busy timeout")?;
    Ok(conn)
}

/// Everything a new `.qrate` file is made of.
///
/// A struct rather than eight positional parameters: `"T", "CSV", None, None, &[], &[], &[]` at a
/// call site names none of its own blanks, and the wizard's `write_project_file` took the same
/// eight in the same order — two signatures to keep in step, and two
/// `#[allow(clippy::too_many_arguments)]` to go with them. `Default` gives a blank project, so a
/// caller writes only the fields it actually has.
#[derive(Default)]
pub struct ProjectSpec<'a> {
    pub name: &'a str,
    pub source: &'a str,
    /// The wizard's Files-step link method, if one was chosen.
    pub link_method: Option<&'a str>,
    /// The wizard's Files-step folder, if one was chosen and linked. Only ever persisted as a
    /// path — qrate never copies the files themselves.
    pub files_folder: Option<&'a str>,
    /// The configured column list, which may differ from `headers` when a column config was loaded.
    pub columns: &'a [ProjectColumn],
    /// The raw spreadsheet. Empty `headers` is a blank project and gets no `dataset_main` table.
    pub headers: &'a [String],
    pub rows: &'a [Vec<String>],
}

/// Creates `path` (a `.qrate` file) with the current schema and imported data.
pub fn create_project_file(path: &Path, spec: &ProjectSpec<'_>) -> Result<()> {
    let &ProjectSpec {
        name,
        source,
        link_method,
        files_folder,
        columns,
        headers,
        rows,
    } = spec;
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
    conn.execute_batch(ROW_STRUCTURE_DDL)
        .context("Create __row_structure")?;
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

    let (headers, row_ids, rows) = qrate_export::read_dataset(&conn)?;
    Ok(ProjectData {
        name,
        columns,
        headers,
        rows,
        row_ids,
        values,
    })
}

/// Reads the optional hierarchy without upgrading an older project. Rows missing from this table
/// are ungrouped roots, so v3 and partially arranged projects remain usable.
pub fn read_row_structure(path: &Path) -> Result<Vec<RowStructure>> {
    let conn = open_ro(path)?;
    qrate_export::read_row_structure(&conn).context("Read row structure")
}

/// Replaces the hierarchy in one transaction. This is the lazy v3-to-v4 migration point: opening
/// an older project stays read-only, while its first structural edit creates the private table.
pub fn write_row_structure(path: &Path, structure: &[RowStructure]) -> Result<()> {
    validate_row_structure(structure)?;
    let mut conn = open_rw(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let tx = conn.transaction().context("Begin row structure update")?;
    tx.execute_batch(ROW_STRUCTURE_DDL)
        .context("Create __row_structure")?;
    tx.execute("DELETE FROM __row_structure", [])
        .context("Clear row structure")?;
    insert_row_structure(&tx, structure)?;
    tx.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
        .context("Set user_version")?;
    tx.commit().context("Commit row structure update")
}

fn insert_row_structure(conn: &Connection, structure: &[RowStructure]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO __row_structure
         (row_id, parent_id, level_key, sibling_order, source_path, source_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for component in structure {
        stmt.execute(params![
            component.row_id,
            component.parent_id,
            component.level_key,
            component.sibling_order,
            component.source_path,
            component.source_kind.map(SourceKind::as_str),
        ])
        .with_context(|| format!("Insert structure for row {}", component.row_id))?;
    }
    Ok(())
}

fn validate_row_structure(structure: &[RowStructure]) -> Result<()> {
    let by_id: HashMap<_, _> = structure.iter().map(|row| (row.row_id, row)).collect();
    if by_id.len() != structure.len() {
        anyhow::bail!("row structure contains duplicate row ids");
    }
    for row in structure {
        if row.level_key.trim().is_empty() {
            anyhow::bail!("row {} has an empty description level", row.row_id);
        }
        let mut parent = row.parent_id;
        let mut remaining = structure.len();
        while let Some(parent_id) = parent {
            if parent_id == row.row_id || remaining == 0 {
                anyhow::bail!("row structure contains a cycle at row {}", row.row_id);
            }
            parent = by_id.get(&parent_id).and_then(|parent| parent.parent_id);
            remaining -= 1;
        }
    }
    Ok(())
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
    write_queued(&open_rw(path)?, key, Some(value))
}

/// One queued write, on `conn`. `None` removes a `__settings` key — a key with no row is already
/// what that asks for. A column notes or column type key upserts only that half of the column's
/// `__columns` row, and gives a column the project was created without a row of its own.
fn write_queued(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    let text = crate::columns::ColumnType::Text.as_str();
    let notes = key.strip_prefix(COLUMN_NOTES_WRITE_PREFIX);
    let data_type = key.strip_prefix(COLUMN_TYPE_WRITE_PREFIX);
    match (notes, data_type, value) {
        (Some(column), _, notes) => conn
            .execute(
                "INSERT INTO __columns(name, data_type, notes) VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO UPDATE SET notes = excluded.notes",
                params![column, text, notes.unwrap_or_default()],
            )
            .context("Upsert column description"),
        (None, Some(column), data_type) => conn
            .execute(
                "INSERT INTO __columns(name, data_type, notes) VALUES (?1, ?2, NULL)
                 ON CONFLICT(name) DO UPDATE SET data_type = excluded.data_type",
                params![column, data_type.unwrap_or(text)],
            )
            .context("Upsert column type"),
        (None, None, Some(value)) => conn
            .execute(
                "INSERT INTO __settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .context("Upsert setting"),
        (None, None, None) => conn
            .execute("DELETE FROM __settings WHERE key = ?1", params![key])
            .context("Delete setting"),
    }?;
    Ok(())
}

/// Every write queued for one file, over one connection in one transaction.
fn write_batch(path: &Path, writes: &[(String, Option<String>)]) -> Result<()> {
    let mut conn = open_rw(path)?;
    let tx = conn.transaction().context("Begin settings update")?;
    for (key, value) in writes {
        write_queued(&tx, key, value.as_deref())?;
    }
    tx.commit().context("Commit settings update")
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
    /// Current zero-based source position, used only by the live table and UI.
    pub row: Option<usize>,
    /// Stable on-disk identity. `None` for a whole-column or whole-dataset note.
    pub row_id: Option<RowId>,
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
/// tolerance [`qrate_export::read_dataset`] gives a blank project's missing `dataset_main`.
pub fn read_notes(path: &Path) -> Result<Vec<StoredNote>> {
    let conn = open_ro(path)?;
    let notes = qrate_export::read_project_notes(&conn)?;
    let row_positions: HashMap<RowId, usize> = if !table_exists(&conn, "dataset_main")? {
        HashMap::new()
    } else {
        let mut ids = conn.prepare("SELECT _row_id FROM dataset_main ORDER BY _row_order")?;
        ids.query_map([], |row| row.get::<_, RowId>(0))?
            .enumerate()
            .map(|(source, id)| id.map(|id| (id, source)))
            .collect::<rusqlite::Result<_>>()?
    };
    Ok(notes
        .into_iter()
        .map(|note| StoredNote {
            row: match note.dataset.as_str() {
                "dataset_main" if !row_positions.is_empty() => {
                    note.row_id.and_then(|id| row_positions.get(&id).copied())
                }
                _ => note.row_id.map(|id| id as usize),
            },
            dataset: note.dataset,
            row_id: note.row_id,
            column: note.column,
            severity: note.severity,
            message: note.message,
            created_at: note.created_at,
            author: note.author,
        })
        .collect())
}

/// Replaces everything `source` previously stored, in one transaction — the same
/// replace-by-source rule the in-memory store uses, so a re-import can't leave orphans behind.
/// Only authored sources belong here (imported notes, user marks); computed diagnostics are
/// recomputed on open and would go stale in storage.
pub fn write_notes(
    path: &Path,
    source: &str,
    notes: &[StoredNote],
    history: &[crate::history::Entry],
) -> Result<()> {
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
                n.row_id,
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
    crate::history::append(&tx, history)?;
    tx.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
        .context("Set user_version")?;
    tx.commit().context("Commit notes")?;
    Ok(())
}

/// Created on first write, like `__notes`, so opening a project stays read-only. Keyed by the path
/// as the caller stores it (relative to the files folder where it can be).
const VISUAL_INDEX_DDL: &str = r#"
    CREATE TABLE IF NOT EXISTS __visual_index (
      path   TEXT PRIMARY KEY,
      len    INTEGER NOT NULL,
      vector BLOB NOT NULL
    );
"#;

/// `__settings` key naming the model that built `__visual_index`. Vectors from different models
/// cannot be compared, so a mismatch reads as an empty index and the next write clears it.
const VISUAL_MODEL_KEY: &str = "visual_model";

/// One image's vector: the stored path, the file's length when embedded, and the vector.
pub type VisualEntry = (String, u64, Vec<f32>);

/// Every vector `model` stored in this project. Empty when there is no index yet, or another model
/// built it.
pub fn read_visual_index(path: &Path, model: &str) -> Result<Vec<VisualEntry>> {
    let conn = open_ro(path)?;
    let exists = table_exists(&conn, "__visual_index")?;
    let built_by: Option<String> = conn
        .query_row(
            "SELECT value FROM __settings WHERE key = ?1",
            params![VISUAL_MODEL_KEY],
            |r| r.get(0),
        )
        .optional()?;
    if !exists || built_by.as_deref() != Some(model) {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT path, len, vector FROM __visual_index")?;
    let rows = stmt.query_map([], |r| {
        let bytes: Vec<u8> = r.get(2)?;
        let vector = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, vector))
    })?;
    rows.collect::<rusqlite::Result<_>>()
        .context("Read visual index")
}

/// Store `entries`, replacing any with the same path, in one transaction. Clears what another model
/// stored first.
pub fn write_visual_index(path: &Path, model: &str, entries: &[VisualEntry]) -> Result<()> {
    let mut conn = open_rw(path)?;
    let tx = conn.transaction()?;
    tx.execute_batch(VISUAL_INDEX_DDL)
        .context("Create __visual_index")?;
    let built_by: Option<String> = tx
        .query_row(
            "SELECT value FROM __settings WHERE key = ?1",
            params![VISUAL_MODEL_KEY],
            |r| r.get(0),
        )
        .optional()?;
    if built_by.as_deref() != Some(model) {
        tx.execute("DELETE FROM __visual_index", [])
            .context("Clear another model's vectors")?;
        tx.execute(
            "INSERT INTO __settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![VISUAL_MODEL_KEY, model],
        )
        .context("Record visual model")?;
    }
    {
        let mut stmt = tx.prepare(
            "INSERT INTO __visual_index(path, len, vector) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET len = excluded.len, vector = excluded.vector",
        )?;
        for (file, len, vector) in entries {
            let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
            stmt.execute(params![file, *len as i64, bytes])
                .context("Insert vector")?;
        }
    }
    tx.commit().context("Commit visual index")?;
    Ok(())
}

/// Debounced background writer for `__settings` values. The dock-layout and
/// window-bounds observers fire on every drag event; latest value per
/// (file, key) wins, and one thread serves all project files — no lifecycle
/// to manage on project switch, since the path travels with each entry.
/// Mirrors `db::SettingsWriter`. A `None` value is a removal, queued like any write.
#[derive(Clone)]
pub struct ProjectSettingsWriter {
    pending: Arc<Mutex<Pending>>,
    wake: mpsc::Sender<()>,
}

type Pending = HashMap<(PathBuf, String), Option<String>>;

impl ProjectSettingsWriter {
    pub fn start() -> Self {
        let pending: Arc<Mutex<Pending>> = Arc::default();
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

    pub fn enqueue(&self, file: &Path, key: &str, value: Option<String>) {
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

    fn write_pending(pending: &Mutex<Pending>) {
        let drained: Vec<_> = match pending.lock() {
            Ok(mut map) => map.drain().collect(),
            Err(_) => return,
        };
        let mut by_file: HashMap<PathBuf, Vec<(String, Option<String>)>> = HashMap::new();
        for ((file, key), value) in drained {
            by_file.entry(file).or_default().push((key, value));
        }
        for (file, writes) in by_file {
            if let Err(err) = write_batch(&file, &writes) {
                log::error!(
                    "failed to save {} project settings to {}: {err:#}",
                    writes.len(),
                    file.display()
                );
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
    queue(file, key, Some(value.to_string()), cx);
}

/// [`queue_write`] for any queued operation, removals included.
fn queue(file: &Path, key: &str, value: Option<String>, cx: &gpui::App) {
    let writer = cx
        .try_global::<ProjectPersistence>()
        .and_then(|p| p.writer.clone());
    match writer {
        Some(w) => w.enqueue(file, key, value),
        None => {
            if let Err(err) = write_batch(file, &[(key.to_string(), value)]) {
                log::error!("failed to save project setting {key}: {err:#}");
            }
        }
    }
}

/// Creates `dataset_main` (one TEXT column per header) and bulk-inserts the rows. Ragged rows
/// (flexible CSV) are padded/truncated to the header count. Manages no transaction of its own —
/// the caller wraps it, so the create and the inserts commit together (and, on a re-save,
/// atomically with the preceding drop).
fn create_and_fill_dataset<S: AsRef<str>>(
    conn: &Connection,
    headers: &[S],
    row_ids: Option<&[RowId]>,
    rows: &[Vec<S>],
) -> Result<()> {
    if row_ids.is_some_and(|ids| ids.len() != rows.len()) {
        anyhow::bail!("row id count does not match dataset row count");
    }
    let idents = dataset_column_idents(headers);
    let cols_sql: Vec<String> = idents.iter().map(|i| format!("{i} TEXT")).collect();
    conn.execute_batch(&format!(
        "CREATE TABLE dataset_main (
            _row_id INTEGER PRIMARY KEY,
            _row_order INTEGER NOT NULL,
            {}
        );",
        cols_sql.join(", ")
    ))
    .context("Create dataset_main")?;

    let mut stmt = conn
        .prepare(&row_insert_sql(&idents, row_ids.is_some()))
        .context("Prepare row insert")?;
    for (ix, row) in rows.iter().enumerate() {
        insert_row(
            &mut stmt,
            row_ids.map(|ids| ids[ix]),
            ix as i64,
            row,
            idents.len(),
        )?;
    }
    Ok(())
}

fn row_insert_sql(idents: &[String], with_id: bool) -> String {
    let first_value = usize::from(with_id) + 2;
    let placeholders: Vec<String> = (first_value..first_value + idents.len())
        .map(|i| format!("?{i}"))
        .collect();
    match with_id {
        true => format!(
            "INSERT INTO dataset_main (_row_id, _row_order, {}) VALUES (?1, ?2, {})",
            idents.join(", "),
            placeholders.join(", ")
        ),
        false => format!(
            "INSERT INTO dataset_main (_row_order, {}) VALUES (?1, {})",
            idents.join(", "),
            placeholders.join(", ")
        ),
    }
}

/// One row through [`row_insert_sql`]'s statement, padded or cut to `width` cells.
fn insert_row<S: AsRef<str>>(
    stmt: &mut rusqlite::Statement<'_>,
    row_id: Option<RowId>,
    order: i64,
    row: &[S],
    width: usize,
) -> Result<()> {
    let cells: Vec<&str> = (0..width)
        .map(|i| row.get(i).map_or("", |cell| cell.as_ref()))
        .collect();
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(width + 2);
    if let Some(id) = &row_id {
        values.push(id);
    }
    values.push(&order);
    values.extend(cells.iter().map(|cell| cell as &dyn rusqlite::ToSql));
    stmt.execute(rusqlite::params_from_iter(values))
        .context("Write row")?;
    Ok(())
}

/// Creates and fills `dataset_main` in one transaction (the create-time path).
fn write_dataset(conn: &Connection, headers: &[String], rows: &[Vec<String>]) -> Result<()> {
    conn.execute_batch("BEGIN")?;
    create_and_fill_dataset(conn, headers, None, rows)?;
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Rewrites `dataset_main` from the in-memory rows — the whole table, in one transaction, so a
/// crash mid-save leaves the previous version intact (an uncommitted transaction rolls back when
/// the connection drops). `headers`/`rows` are written in the order given, which becomes the
/// file's physical column order. A blank project (no headers, no `dataset_main`) is a no-op.
///
/// `structure` is the live hierarchy, written in the same transaction as the rows it names;
/// `None` keeps whatever hierarchy the file already holds.
pub fn save_dataset(
    path: &Path,
    headers: &[String],
    row_ids: &[RowId],
    rows: &[Vec<String>],
    structure: Option<&[RowStructure]>,
    history: &[crate::history::Entry],
) -> Result<()> {
    if headers.is_empty() {
        return Ok(());
    }
    rewrite_dataset(open_rw(path)?, headers, row_ids, rows, structure, history)
}

/// [`save_dataset`] that writes only what `history` says changed: the rows it names are updated
/// or inserted, rows no longer present are deleted and the rest only have their order touched, all
/// by `_row_id` in one transaction. `history` must be every entry since the file last saved —
/// the same entries this appends to the log.
///
/// Falls back to the full rewrite when the column set differs from the file's (a column added,
/// removed, renamed or moved), since that is a new table rather than new rows.
pub fn save_changes<S: AsRef<str>>(
    path: &Path,
    headers: &[S],
    row_ids: &[RowId],
    rows: &[Vec<S>],
    structure: Option<&[RowStructure]>,
    history: &[crate::history::Entry],
) -> Result<()> {
    use crate::history::Change;

    if headers.is_empty() {
        return Ok(());
    }
    if row_ids.len() != rows.len() {
        anyhow::bail!("row id count does not match dataset row count");
    }
    let mut conn = open_rw(path)?;
    let reshaped = history
        .iter()
        .flat_map(|entry| &entry.changes)
        .any(|change| {
            matches!(
                change,
                Change::ColumnAdded { .. }
                    | Change::ColumnRemoved { .. }
                    | Change::ColumnRenamed { .. }
                    | Change::ColumnMoved { .. }
            )
        });
    let names = dataset_column_names(headers);
    if reshaped || stored_columns(&conn)?.as_ref() != Some(&names) {
        return rewrite_dataset(conn, headers, row_ids, rows, structure, history);
    }
    let touched: HashSet<RowId> = history
        .iter()
        .flat_map(|entry| &entry.changes)
        .filter_map(|change| match change {
            Change::Cell { row, .. } | Change::RowAdded { row, .. } => Some(*row),
            _ => None,
        })
        .collect();
    let structure = planned_structure(&conn, structure, row_ids)?;

    let tx = conn.transaction().context("Begin dataset update")?;
    let stored: HashMap<RowId, i64> = tx
        .prepare("SELECT _row_id, _row_order FROM dataset_main")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()
        .context("Read stored rows")?;
    let kept: HashSet<RowId> = row_ids.iter().copied().collect();
    {
        let mut delete = tx.prepare("DELETE FROM dataset_main WHERE _row_id = ?1")?;
        for id in stored.keys().filter(|id| !kept.contains(id)) {
            delete.execute([id]).context("Delete row")?;
        }
        let idents = dataset_column_idents(headers);
        let mut insert = tx.prepare(&row_insert_sql(&idents, true))?;
        let assignments: Vec<String> = idents
            .iter()
            .enumerate()
            .map(|(i, ident)| format!("{ident} = ?{}", i + 3))
            .collect();
        let mut update = tx.prepare(&format!(
            "UPDATE dataset_main SET _row_order = ?2, {} WHERE _row_id = ?1",
            assignments.join(", ")
        ))?;
        let mut reorder =
            tx.prepare("UPDATE dataset_main SET _row_order = ?2 WHERE _row_id = ?1")?;
        for (ix, (id, row)) in row_ids.iter().zip(rows).enumerate() {
            let order = ix as i64;
            match stored.get(id) {
                None => insert_row(&mut insert, Some(*id), order, row, idents.len())?,
                Some(_) if touched.contains(id) => {
                    insert_row(&mut update, Some(*id), order, row, idents.len())?
                }
                Some(was) if *was != order => {
                    reorder.execute(params![id, order]).context("Reorder row")?;
                }
                Some(_) => {}
            }
        }
    }
    if let Some(structure) = structure {
        tx.execute_batch(ROW_STRUCTURE_DDL)
            .context("Create __row_structure")?;
        let stored: HashMap<RowId, RowStructure> = qrate_export::read_row_structure(&tx)?
            .into_iter()
            .map(|row| (row.row_id, row))
            .collect();
        let wanted: HashSet<RowId> = structure.iter().map(|row| row.row_id).collect();
        {
            let mut delete = tx.prepare("DELETE FROM __row_structure WHERE row_id = ?1")?;
            for id in stored.keys().filter(|id| !wanted.contains(id)) {
                delete.execute([id]).context("Delete row structure")?;
            }
        }
        let changed: Vec<RowStructure> = structure
            .into_iter()
            .filter(|row| stored.get(&row.row_id) != Some(row))
            .collect();
        upsert_row_structure(&tx, &changed)?;
        tx.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
            .context("Set user_version")?;
    }
    crate::history::append(&tx, history)?;
    tx.commit().context("Commit dataset update")
}

/// The whole-table half of [`save_dataset`] and [`save_changes`]: drop and recreate.
fn rewrite_dataset<S: AsRef<str>>(
    mut conn: Connection,
    headers: &[S],
    row_ids: &[RowId],
    rows: &[Vec<S>],
    structure: Option<&[RowStructure]>,
    history: &[crate::history::Entry],
) -> Result<()> {
    let structure = planned_structure(&conn, structure, row_ids)?;
    let tx = conn.transaction().context("Begin dataset rewrite")?;
    tx.execute_batch("DROP TABLE IF EXISTS __row_structure; DROP TABLE IF EXISTS dataset_main;")
        .context("Begin dataset rewrite")?;
    create_and_fill_dataset(&tx, headers, Some(row_ids), rows)?;
    if let Some(structure) = structure {
        tx.execute_batch(ROW_STRUCTURE_DDL)
            .context("Recreate __row_structure")?;
        insert_row_structure(&tx, &structure)?;
        tx.pragma_update(None, "user_version", QRATE_SCHEMA_VERSION)
            .context("Set user_version")?;
    }
    crate::history::append(&tx, history)?;
    tx.commit().context("Commit dataset rewrite")
}

/// The hierarchy a save writes, normalized to the rows being saved, or `None` when there is none
/// to write: a flat v3 project stays v3 until it has an arrangement worth storing.
fn planned_structure(
    conn: &Connection,
    structure: Option<&[RowStructure]>,
    row_ids: &[RowId],
) -> Result<Option<Vec<RowStructure>>> {
    let had_structure = table_exists(conn, "__row_structure")?;
    let structure = match structure {
        Some(structure) => structure.to_vec(),
        None if had_structure => qrate_export::read_row_structure(conn)?,
        None => Vec::new(),
    };
    let structure = retained_row_structure(structure, row_ids);
    if !had_structure && is_flat(&structure, row_ids) {
        return Ok(None);
    }
    validate_row_structure(&structure)?;
    Ok(Some(structure))
}

fn upsert_row_structure(conn: &Connection, structure: &[RowStructure]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO __row_structure
         (row_id, parent_id, level_key, sibling_order, source_path, source_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for component in structure {
        stmt.execute(params![
            component.row_id,
            component.parent_id,
            component.level_key,
            component.sibling_order,
            component.source_path,
            component.source_kind.map(SourceKind::as_str),
        ])
        .with_context(|| format!("Update structure for row {}", component.row_id))?;
    }
    Ok(())
}

/// `dataset_main`'s data columns as the file holds them, in physical order. `None` without one.
fn stored_columns(conn: &Connection) -> Result<Option<Vec<String>>> {
    if !table_exists(conn, "dataset_main")? {
        return Ok(None);
    }
    let names = conn
        .prepare("SELECT name FROM pragma_table_info('dataset_main') ORDER BY cid")?
        .query_map([], |r| r.get::<_, String>(0))?
        .filter(|name| {
            !name
                .as_ref()
                .is_ok_and(|name| name == "_row_id" || name == "_row_order")
        })
        .collect::<rusqlite::Result<_>>()
        .context("Read dataset columns")?;
    Ok(Some(names))
}

fn is_flat(structure: &[RowStructure], row_ids: &[RowId]) -> bool {
    let Some(first) = structure.first() else {
        return true;
    };
    let position: HashMap<_, _> = row_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut by_order: Vec<_> = structure.iter().collect();
    by_order.sort_by_key(|row| (row.sibling_order, row.row_id));
    structure.iter().all(|row| {
        row.parent_id.is_none() && row.source_path.is_none() && row.level_key == first.level_key
    }) && by_order
        .windows(2)
        .all(|pair| position.get(&pair[0].row_id) < position.get(&pair[1].row_id))
}

fn retained_row_structure(structure: Vec<RowStructure>, row_ids: &[RowId]) -> Vec<RowStructure> {
    let retained: HashSet<_> = row_ids.iter().copied().collect();
    let mut structure: Vec<_> = structure
        .into_iter()
        .filter(|row| retained.contains(&row.row_id))
        .map(|mut row| {
            if row.parent_id.is_some_and(|id| !retained.contains(&id)) {
                row.parent_id = None;
            }
            row
        })
        .collect();
    structure.sort_by_key(|row| (row.parent_id, row.sibling_order, row.row_id));
    let mut next_order = HashMap::<Option<RowId>, i64>::new();
    for row in &mut structure {
        let order = next_order.entry(row.parent_id).or_default();
        row.sibling_order = *order;
        *order += 1;
    }
    structure
}

/// Each header as the SQL column it is stored under, de-duplicating case-insensitively
/// (`Title`, `title` → `Title`, `title_2`) and naming blanks `column_N` — spreadsheet headers are
/// user data and can collide or be empty.
fn dataset_column_names<S: AsRef<str>>(headers: &[S]) -> Vec<String> {
    let mut seen = std::collections::HashSet::from(["_row_id".into(), "_row_order".into()]);
    headers
        .iter()
        .enumerate()
        .map(|(ix, h)| {
            let base = h.as_ref().trim();
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
            name
        })
        .collect()
}

/// [`dataset_column_names`], quoted as SQL identifiers.
fn dataset_column_idents<S: AsRef<str>>(headers: &[S]) -> Vec<String> {
    dataset_column_names(headers)
        .iter()
        .map(|name| quote_identifier(name))
        .collect()
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
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
            row_id: row.map(|row| row as RowId),
            column: column.map(str::to_string),
            severity: "note".into(),
            message: msg.into(),
            created_at: None,
            author: None,
        }
    }

    fn blank_project(path: &Path) {
        create_project_file(
            path,
            &ProjectSpec {
                name: "Notes",
                source: "CSV",
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn visual_index_round_trips_and_forgets_another_models_vectors() {
        let path = tempfile("visual.qrate");
        blank_project(&path);
        assert!(read_visual_index(&path, "clip-a").unwrap().is_empty());

        write_visual_index(&path, "clip-a", &[("a.jpg".into(), 6, vec![0.6, 0.8])]).unwrap();
        write_visual_index(&path, "clip-a", &[("a.jpg".into(), 7, vec![1.0, 0.0])]).unwrap();
        assert_eq!(
            read_visual_index(&path, "clip-a").unwrap(),
            vec![("a.jpg".to_string(), 7, vec![1.0, 0.0])],
            "a re-embedded file replaces its row"
        );
        assert!(read_visual_index(&path, "clip-b").unwrap().is_empty());

        write_visual_index(&path, "clip-b", &[("b.jpg".into(), 1, vec![0.0])]).unwrap();
        assert_eq!(read_visual_index(&path, "clip-b").unwrap().len(), 1);
        assert!(read_visual_index(&path, "clip-a").unwrap().is_empty());
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
        write_notes(&path, "import", &[fresh], &[]).unwrap();

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
            &[],
        )
        .unwrap();
        assert_eq!(read_notes(&path).unwrap().len(), 2);

        // A different source is additive — it must not disturb `import`'s entries.
        write_notes(
            &path,
            "spell",
            &[note(None, Some("Title"), "whole column")],
            &[],
        )
        .unwrap();
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
        write_notes(&path, "import", &[], &[]).unwrap();
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
        write_notes(&path, "import", &[note(Some(1), Some("A"), "hi")], &[]).unwrap();
        assert_eq!(read_notes(&path).unwrap().len(), 1);
    }

    /// The wizard writes `__columns` once and nothing has updated it since, so both halves matter:
    /// retyping a configured column, and typing one the wizard never wrote a row for.
    #[test]
    fn a_column_type_is_upserted_and_survives_a_reload() {
        let path = tempfile("types.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "Types",
                source: "CSV + folder",
                columns: &[ProjectColumn {
                    name: "Digital ID".into(),
                    data_type: "Text".into(),
                    notes: "the scan".into(),
                }],
                headers: &["Digital ID".to_string()],
                rows: &[vec!["1".to_string()]],
                ..Default::default()
            },
        )
        .unwrap();

        let queued = |key: &str, value: &str| (key.to_string(), Some(value.to_string()));
        write_batch(
            &path,
            &[
                queued("column_type:Digital ID", "Filename"),
                queued("column_type:Taken", "Date"),
                queued("column_notes:Digital ID", "the master scan filename"),
                queued("column_notes:Taken", "capture date"),
            ],
        )
        .unwrap();

        let columns = load_project_file(&path).unwrap().columns;
        let by_name = |n: &str| {
            columns
                .iter()
                .find(|c| c.name == n)
                .map(|c| (c.data_type.as_str(), c.notes.as_str()))
        };
        assert_eq!(
            by_name("Digital ID"),
            Some(("Filename", "the master scan filename"))
        );
        assert_eq!(by_name("Taken"), Some(("Date", "capture date")));
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
            &ProjectSpec {
                name: "Test Project",
                source: "CSV + folder",
                link_method: Some("exact filename"),
                files_folder: Some("/photos"),
                columns: &columns,
                headers: &headers,
                rows: &rows,
            },
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
        let conn = Connection::open(&path).unwrap();
        let shared = qrate_export::read_dataset(&conn).unwrap();
        assert_eq!(
            shared,
            (
                data.headers.clone(),
                data.row_ids.clone(),
                data.rows.clone()
            )
        );
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
        create_project_file(
            &path,
            &ProjectSpec {
                name: "S",
                source: "CSV",
                headers: &headers,
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();

        // Edit a cell and add a row, then save the whole table back.
        let edited = vec![
            vec!["1".to_string(), "Edited".to_string()],
            vec!["2".to_string(), "Second".to_string()],
        ];
        save_dataset(&path, &headers, &[1, 2], &edited, None, &[]).unwrap();

        let data = load_project_file(&path).unwrap();
        assert_eq!(data.headers, headers);
        assert_eq!(data.rows, edited);
        assert_eq!(data.row_ids, vec![1, 2]);
        // DELETE mode's invariant survives the rewrite: only the `.qrate` file at rest.
        assert!(!path.with_extension("qrate-journal").exists());
        assert!(!path.with_extension("qrate-wal").exists());
    }

    /// Saves `after` both ways — incrementally from `changes`, and as a full rewrite of a copy —
    /// starting from the same three-row file, then reopens both and compares them.
    fn saves_like_a_rewrite(
        name: &str,
        headers: &[&str],
        row_ids: &[RowId],
        after: &[&[&str]],
        changes: Vec<crate::history::Change>,
    ) -> ProjectData {
        let incremental = tempfile(&format!("{name}-changes.qrate"));
        let rewritten = tempfile(&format!("{name}-rewrite.qrate"));
        for path in [&incremental, &rewritten] {
            create_project_file(
                path,
                &ProjectSpec {
                    name: "Incremental",
                    source: "CSV",
                    headers: &["Title".to_string(), "Date".to_string()],
                    rows: &[
                        vec!["one".into(), "1901".into()],
                        vec!["two".into(), "1902".into()],
                        vec!["three".into(), "1903".into()],
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let headers: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
        let rows: Vec<Vec<String>> = after
            .iter()
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect();
        let log = [crate::history::Entry::new(
            crate::history::Origin::Typed,
            changes,
            None,
        )];
        save_changes(&incremental, &headers, row_ids, &rows, None, &log).unwrap();
        save_dataset(&rewritten, &headers, row_ids, &rows, None, &log).unwrap();

        let (a, b) = (
            load_project_file(&incremental).unwrap(),
            load_project_file(&rewritten).unwrap(),
        );
        assert_eq!(
            (&a.headers, &a.row_ids, &a.rows),
            (&b.headers, &b.row_ids, &b.rows)
        );
        assert_eq!(
            read_row_structure(&incremental).unwrap(),
            read_row_structure(&rewritten).unwrap()
        );
        assert_eq!(
            crate::history::entries_after(&incremental, 0)
                .unwrap()
                .len(),
            1,
            "the log entry lands with the data"
        );
        a
    }

    #[test]
    fn an_incremental_cell_edit_matches_a_rewrite() {
        let data = saves_like_a_rewrite(
            "cell",
            &["Title", "Date"],
            &[1, 2, 3],
            &[&["one", "1901"], &["TWO", "1902"], &["three", "1903"]],
            vec![crate::history::Change::Cell {
                row: 2,
                column: "Title".into(),
                before: "two".into(),
                after: "TWO".into(),
            }],
        );
        assert_eq!(data.rows[1], vec!["TWO", "1902"]);
    }

    #[test]
    fn an_incremental_row_insert_matches_a_rewrite() {
        let data = saves_like_a_rewrite(
            "insert",
            &["Title", "Date"],
            &[4, 1, 2, 3],
            &[
                &["new", ""],
                &["one", "1901"],
                &["two", "1902"],
                &["three", "1903"],
            ],
            vec![crate::history::Change::RowAdded {
                row: 4,
                position: 0,
                cells: vec![("Title".into(), "new".into())],
            }],
        );
        assert_eq!(data.row_ids, vec![4, 1, 2, 3]);
    }

    #[test]
    fn an_incremental_row_delete_matches_a_rewrite() {
        let data = saves_like_a_rewrite(
            "delete",
            &["Title", "Date"],
            &[1, 3],
            &[&["one", "1901"], &["three", "1903"]],
            vec![crate::history::Change::RowRemoved {
                row: 2,
                position: 1,
                cells: vec![("Title".into(), "two".into())],
            }],
        );
        assert_eq!(data.row_ids, vec![1, 3]);
    }

    /// A new column is a new table, so the save falls back to rewriting it — and still agrees.
    #[test]
    fn an_added_column_rewrites_the_table() {
        let data = saves_like_a_rewrite(
            "column",
            &["Title", "Place", "Date"],
            &[1, 2, 3],
            &[
                &["one", "", "1901"],
                &["two", "Hope", "1902"],
                &["three", "", "1903"],
            ],
            vec![crate::history::Change::ColumnAdded {
                column: "Place".into(),
                position: 1,
                cells: Vec::new(),
            }],
        );
        assert_eq!(data.headers, vec!["Title", "Place", "Date"]);
        assert_eq!(data.rows[1], vec!["two", "Hope", "1902"]);
    }

    #[test]
    fn row_structure_round_trips_and_rejects_cycles() {
        let path = tempfile("structure.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "Structure",
                source: "Folder",
                headers: &["Title".into()],
                rows: &[vec!["Series".into()], vec!["Item".into()]],
                ..Default::default()
            },
        )
        .unwrap();
        let structure = vec![
            RowStructure {
                row_id: 1,
                parent_id: None,
                level_key: "series".into(),
                sibling_order: 0,
                source_path: Some("photographs".into()),
                source_kind: Some(SourceKind::Directory),
            },
            RowStructure {
                row_id: 2,
                parent_id: Some(1),
                level_key: "item".into(),
                sibling_order: 0,
                source_path: Some("photographs/001.jpg".into()),
                source_kind: Some(SourceKind::File),
            },
        ];
        write_row_structure(&path, &structure).unwrap();
        assert_eq!(read_row_structure(&path).unwrap(), structure);

        let cycle = vec![
            RowStructure {
                parent_id: Some(2),
                ..structure[0].clone()
            },
            structure[1].clone(),
        ];
        assert!(write_row_structure(&path, &cycle).is_err());
        assert_eq!(read_row_structure(&path).unwrap(), structure);
    }

    #[test]
    fn opening_v3_does_not_create_structure_until_its_first_write() {
        let path = tempfile("lazy-v4.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "Legacy",
                source: "CSV",
                headers: &["Title".into()],
                rows: &[vec!["Item".into()]],
                ..Default::default()
            },
        )
        .unwrap();
        {
            let conn = open_rw(&path).unwrap();
            conn.execute_batch("DROP TABLE __row_structure; PRAGMA user_version = 3")
                .unwrap();
        }

        let root = |row_id, sibling_order| RowStructure {
            row_id,
            parent_id: None,
            level_key: "item".into(),
            sibling_order,
            source_path: None,
            source_kind: None,
        };
        let headers = ["Title".to_string()];
        assert!(read_row_structure(&path).unwrap().is_empty());
        save_dataset(
            &path,
            &headers,
            &[1],
            &[vec!["Edited".into()]],
            Some(&[root(1, 0)]),
            &[],
        )
        .unwrap();
        let conn = open_ro(&path).unwrap();
        assert!(!table_exists(&conn, "__row_structure").unwrap());
        drop(conn);

        // The child is new in this save: its structure has to land with its row, not before it.
        let nested = [
            root(1, 0),
            RowStructure {
                parent_id: Some(1),
                ..root(2, 0)
            },
        ];
        save_dataset(
            &path,
            &headers,
            &[1, 2],
            &[vec!["Series".into()], vec!["Child".into()]],
            Some(&nested),
            &[],
        )
        .unwrap();
        assert_eq!(read_row_structure(&path).unwrap(), nested);
        let conn = open_ro(&path).unwrap();
        assert!(table_exists(&conn, "__row_structure").unwrap());
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
                .unwrap(),
            4
        );
    }

    #[test]
    fn dataset_save_preserves_retained_structure_and_promotes_orphans() {
        let path = tempfile("save-structure.qrate");
        let headers = vec!["Title".into()];
        create_project_file(
            &path,
            &ProjectSpec {
                name: "Save structure",
                source: "Folder",
                headers: &headers,
                rows: &[vec!["Series".into()], vec!["A".into()], vec!["B".into()]],
                ..Default::default()
            },
        )
        .unwrap();
        write_row_structure(
            &path,
            &[
                RowStructure {
                    row_id: 1,
                    parent_id: None,
                    level_key: "series".into(),
                    sibling_order: 0,
                    source_path: None,
                    source_kind: None,
                },
                RowStructure {
                    row_id: 2,
                    parent_id: Some(1),
                    level_key: "item".into(),
                    sibling_order: 0,
                    source_path: None,
                    source_kind: None,
                },
                RowStructure {
                    row_id: 3,
                    parent_id: Some(1),
                    level_key: "item".into(),
                    sibling_order: 1,
                    source_path: None,
                    source_kind: None,
                },
            ],
        )
        .unwrap();

        save_dataset(
            &path,
            &headers,
            &[2, 3],
            &[vec!["A".into()], vec!["B".into()]],
            None,
            &[],
        )
        .unwrap();
        let structure = read_row_structure(&path).unwrap();
        assert_eq!(structure.len(), 2);
        assert!(structure.iter().all(|row| row.parent_id.is_none()));
        assert_eq!(
            structure
                .iter()
                .map(|row| row.sibling_order)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn notes_follow_stable_rows_through_a_top_insert_and_reload() {
        let path = tempfile("stable-row-notes.qrate");
        let headers = vec!["Title".to_string()];
        let rows: Vec<_> = (0..6).map(|i| vec![format!("item {i}")]).collect();
        create_project_file(
            &path,
            &ProjectSpec {
                name: "S",
                source: "CSV",
                headers: &headers,
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();
        write_notes(
            &path,
            "note",
            &[
                StoredNote {
                    dataset: "dataset_main".into(),
                    row: Some(2),
                    row_id: Some(3),
                    column: Some("Title".into()),
                    severity: "note".into(),
                    message: "on item 2".into(),
                    created_at: None,
                    author: None,
                },
                StoredNote {
                    dataset: "dataset_main".into(),
                    row: Some(5),
                    row_id: Some(6),
                    column: Some("Title".into()),
                    severity: "note".into(),
                    message: "on item 5".into(),
                    created_at: None,
                    author: None,
                },
            ],
            &[],
        )
        .unwrap();

        let mut inserted = vec![vec!["new item".into()]];
        inserted.extend(rows);
        save_dataset(
            &path,
            &headers,
            &[7, 1, 2, 3, 4, 5, 6],
            &inserted,
            None,
            &[],
        )
        .unwrap();

        let reopened = load_project_file(&path).unwrap();
        assert_eq!(reopened.row_ids, vec![7, 1, 2, 3, 4, 5, 6]);
        let notes = read_notes(&path).unwrap();
        let attached: Vec<_> = notes
            .iter()
            .map(|note| {
                (
                    note.message.as_str(),
                    reopened.rows[note.row.unwrap()][0].as_str(),
                )
            })
            .collect();
        assert_eq!(
            attached,
            vec![("on item 2", "item 2"), ("on item 5", "item 5")]
        );
    }

    #[test]
    fn load_project_file_caches_settings_values() {
        let path = tempfile("values.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "V",
                source: "Blank",
                ..Default::default()
            },
        )
        .unwrap();
        write_setting(&path, "table_stripes", "true").unwrap();

        let data = load_project_file(&path).unwrap();
        assert_eq!(data.name, "V");
        assert!(data.values.get("table_stripes").unwrap().bool());
        // Creation-time settings are cached too.
        assert_eq!(data.values.get("source").unwrap().text(), "Blank");
        assert!(!data.values.contains_key("table_stripes_missing"));
    }

    /// Clearing an override has to remove the row, not blank it: a present-but-empty value still
    /// reads as "this project says so" and would keep shadowing the user-wide default forever.
    #[test]
    fn clearing_an_override_removes_the_row() {
        let path = tempfile("clear.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "C",
                source: "Blank",
                ..Default::default()
            },
        )
        .unwrap();
        write_setting(&path, "table_stripes", "true").unwrap();
        assert!(
            load_project_file(&path)
                .unwrap()
                .values
                .contains_key("table_stripes")
        );

        write_batch(&path, &[("table_stripes".into(), None)]).unwrap();
        assert!(
            !load_project_file(&path)
                .unwrap()
                .values
                .contains_key("table_stripes")
        );
        // Clearing what was never set is what the caller asked for, not a failure.
        write_batch(&path, &[("table_stripes".into(), None)]).unwrap();
    }

    #[test]
    fn writer_flushes_latest_value_per_key() {
        let path = tempfile("writer.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "W",
                source: "Blank",
                ..Default::default()
            },
        )
        .unwrap();

        let writer = ProjectSettingsWriter::start();
        writer.enqueue(&path, "dock_layout", Some("{\"v\":1}".into()));
        writer.enqueue(&path, "dock_layout", Some("{\"v\":2}".into())); // latest wins
        writer.flush(); // quit path: synchronous, doesn't wait out the debounce
        assert_eq!(
            read_setting(&path, "dock_layout").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
    }

    #[test]
    fn blank_project_has_no_dataset_table() {
        let path = tempfile("blank.qrate");
        create_project_file(
            &path,
            &ProjectSpec {
                name: "Blank",
                source: "Blank",
                ..Default::default()
            },
        )
        .unwrap();
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
