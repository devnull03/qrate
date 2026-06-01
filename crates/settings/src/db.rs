//! Persistent storage for `AppSettings`.
//!
//! Implementation notes:
//! - Stores one JSON blob in SQLite (single-row KV table).
//! - Writes are debounced in a background thread; mutations enqueue snapshots.
//! - Includes optional main window size and display for restore on launch (not position).
//! - JSON includes `settings_version` for forward-compatible migrations.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result};
use gpui::SharedString;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{
    AppSettings, MainWindowBounds, Val, SETTINGS_SCHEMA_VERSION,
};

const APP_DIR: &str = "qrate";
const DB_FILE: &str = "settings.sqlite3";
const SETTINGS_KEY: &str = "app_settings_v1";

#[derive(Clone, Serialize, Deserialize)]
enum PersistVal {
    Text(String),
    Bool(bool),
}

fn default_persist_settings_version() -> u32 {
    1
}

#[derive(Clone, Serialize, Deserialize)]
struct PersistSettings {
    /// Schema version of this blob; missing in older files defaults to `1`.
    #[serde(default = "default_persist_settings_version")]
    settings_version: u32,
    values: HashMap<String, PersistVal>,
    #[serde(default)]
    main_window_bounds: Option<MainWindowBounds>,
}

impl From<&AppSettings> for PersistSettings {
    fn from(s: &AppSettings) -> Self {
        let values = s
            .values
            .iter()
            .map(|(k, v)| {
                let pv = match v {
                    Val::Text(t) => PersistVal::Text(t.to_string()),
                    Val::Bool(b) => PersistVal::Bool(*b),
                };
                (k.clone(), pv)
            })
            .collect();

        Self {
            settings_version: SETTINGS_SCHEMA_VERSION,
            values,
            main_window_bounds: s.main_window_bounds.clone(),
        }
    }
}

impl From<PersistSettings> for AppSettings {
    fn from(p: PersistSettings) -> Self {
        let values = p
            .values
            .into_iter()
            .map(|(k, v)| {
                let vv = match v {
                    PersistVal::Text(t) => Val::Text(SharedString::from(t)),
                    PersistVal::Bool(b) => Val::Bool(b),
                };
                (k, vv)
            })
            .collect();

        Self {
            values,
            main_window_bounds: p.main_window_bounds,
            settings_schema_version: p.settings_version,
        }
    }
}


fn db_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir().context("Failed to resolve local data dir")?.join(APP_DIR);
    Ok(base.join(DB_FILE))
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings_kv (
          key TEXT PRIMARY KEY,
          json TEXT NOT NULL
        );
        "#,
    )
    .context("Failed to create schema")?;
    Ok(())
}

fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("Create dir {parent:?}"))?;
    }
    let conn = Connection::open(path).with_context(|| format!("Open DB at {path:?}"))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

pub fn load_app_settings() -> Result<AppSettings> {
    let path = db_path()?;
    let conn = open_db(&path)?;
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM settings_kv WHERE key = ?1",
            params![SETTINGS_KEY],
            |row| row.get(0),
        )
        .optional()
        .context("Query settings")?;

    let Some(json) = json else {
        return Ok(AppSettings::default());
    };

    let persist: PersistSettings =
        serde_json::from_str(&json).context("Deserialize persisted settings")?;
    Ok(persist.into())
}

fn save_app_settings_snapshot(snapshot: PersistSettings) -> Result<()> {
    let path = db_path()?;
    let conn = open_db(&path)?;
    let json = serde_json::to_string(&snapshot).context("Serialize settings")?;
    conn.execute(
        "INSERT INTO settings_kv(key, json) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET json = excluded.json",
        params![SETTINGS_KEY, json],
    )
    .context("Upsert settings row")?;
    Ok(())
}

/// Background writer handle. Call `enqueue_save` on every mutation; writes are debounced.
#[derive(Clone)]
pub struct SettingsWriter {
    tx: mpsc::Sender<PersistSettings>,
}

impl SettingsWriter {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<PersistSettings>();

        thread::spawn(move || {
            let debounce = Duration::from_millis(450);
            let mut pending: Option<PersistSettings> = None;

            loop {
                match rx.recv_timeout(debounce) {
                    Ok(s) => {
                        pending = Some(s);
                        // Drain bursts quickly.
                        while let Ok(s2) = rx.try_recv() {
                            pending = Some(s2);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(s) = pending.take() {
                            let _ = save_app_settings_snapshot(s);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(s) = pending.take() {
                            let _ = save_app_settings_snapshot(s);
                        }
                        break;
                    }
                }
            }
        });

        Self { tx }
    }

    pub fn enqueue_save(&self, settings: &AppSettings) {
        let _ = self.tx.send(PersistSettings::from(settings));
    }
}
