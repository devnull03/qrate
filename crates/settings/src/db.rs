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
    time::Duration,
};

use anyhow::{Context as _, Result};
use gpui::{BackgroundExecutor, SharedString};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{AppSettings, MainWindowBounds, SETTINGS_SCHEMA_VERSION, Val};

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

/// qrate's own directory under the platform's local data dir. Everything the app writes outside a
/// project file lives here — the settings database, and the plugins folder.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|base| base.join(APP_DIR))
}

fn db_path() -> Result<PathBuf> {
    let base = data_dir().context("Failed to resolve local data dir")?;
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
        log::debug!("settings: no persisted app settings found");
        return Ok(AppSettings::default());
    };

    let persist: PersistSettings =
        serde_json::from_str(&json).context("Deserialize persisted settings")?;
    log::debug!(
        "settings: loaded app settings schema={} keys={}",
        persist.settings_version,
        persist.values.len()
    );
    Ok(persist.into())
}

/// Writes the settings synchronously, bypassing the debounced writer — the
/// app-quit path, where a pending 450ms debounce would otherwise be lost.
pub fn flush_app_settings(settings: &AppSettings) -> Result<()> {
    save_app_settings_snapshot(PersistSettings::from(settings))
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
    log::debug!(
        "settings: saved app snapshot keys={}",
        snapshot.values.len()
    );
    Ok(())
}

/// Background writer handle. Call `enqueue_save` on every mutation; writes are debounced, and the
/// quit path calls [`flush_app_settings`] instead of waiting the debounce out.
#[derive(Clone)]
pub struct SettingsWriter {
    tx: async_channel::Sender<PersistSettings>,
}

impl SettingsWriter {
    pub fn start(executor: &BackgroundExecutor) -> Self {
        let (tx, rx) = async_channel::unbounded::<PersistSettings>();
        let timer = executor.clone();
        executor
            .spawn(async move {
                while let Ok(mut pending) = rx.recv().await {
                    // Latest snapshot wins once a whole debounce window passes without a newer one.
                    loop {
                        timer.timer(Duration::from_millis(450)).await;
                        let Ok(newer) = rx.try_recv() else { break };
                        pending = newer;
                        while let Ok(newer) = rx.try_recv() {
                            pending = newer;
                        }
                    }
                    if let Err(error) = save_app_settings_snapshot(pending) {
                        log::error!("settings: failed to save app snapshot: {error:#}");
                    }
                }
            })
            .detach();
        Self { tx }
    }

    pub fn enqueue_save(&self, settings: &AppSettings) {
        if self.tx.try_send(PersistSettings::from(settings)).is_err() {
            log::error!("settings: app writer is unavailable");
        }
    }
}
