//! One-off migration: re-key a `.qrate` file's per-column settings from the old positional
//! `c{ix}` keys to the column names qrate uses now (see `settings::columns`).
//!
//! Positional keys were re-minted from the on-disk column order on every project open, so deleting
//! a column would have slid every later column's preferences onto its neighbour. Deliberately a
//! script and not a startup migration: it runs once, over the projects that exist today, and
//! leaving it in the app would mean carrying the old convention forever.
//!
//! ```text
//! cargo run -p settings --example migrate_column_keys              # every recent project
//! cargo run -p settings --example migrate_column_keys -- a.qrate   # or named files
//! ```
//!
//! Writes a `.qrate.bak` beside each file first, and says what it changed.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

/// The `__settings` keys holding a column key: the settings map, and the saved column layout.
const COLUMN_SETTINGS_KEY: &str = "table_column_settings";
const COLUMN_LAYOUT_KEY: &str = "table_columns";

fn main() -> Result<()> {
    let mut files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        files = recents().context("Read the recent-projects list")?;
        println!("no files given — migrating {} recent projects", files.len());
    }
    for file in &files {
        let path = Path::new(file);
        match migrate(path) {
            Ok(report) => println!("{}: {report}", path.display()),
            Err(err) => eprintln!("{}: {err:#}", path.display()),
        }
    }
    Ok(())
}

/// Every project on the launcher's recent list, read straight out of the app's settings database —
/// the projects that exist on this machine, which is exactly the set that needs migrating.
fn recents() -> Result<Vec<String>> {
    let db = settings::data_dir()
        .context("No local data dir")?
        .join("settings.sqlite3");
    let conn = Connection::open(&db).context("Open settings database")?;
    // Every app setting lives in one blob under `app_settings_v1`; the recent list is one of its
    // values, itself JSON, hence the second parse.
    let raw: Option<String> = conn
        .query_row(
            "SELECT json FROM settings_kv WHERE key = 'app_settings_v1'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let blob: serde_json::Value = serde_json::from_str(raw.as_deref().unwrap_or("{}"))?;
    let list = blob["values"]["project_wizard.recent_projects"]["Text"]
        .as_str()
        .unwrap_or("[]");
    let entries: Vec<serde_json::Value> = serde_json::from_str(list).unwrap_or_default();
    Ok(entries
        .iter()
        .filter_map(|e| e.get("path")?.as_str().map(str::to_string))
        .collect())
}

fn migrate(path: &Path) -> Result<String> {
    let conn = Connection::open(path).context("Open project")?;
    let headers = headers(&conn)?;
    if headers.is_empty() {
        return Ok("no dataset_main — nothing to migrate".into());
    }

    // `c{N}` addressed the column at physical position N, which is the order `dataset_main`
    // declares its columns in.
    let name_of = |key: &str| -> Option<String> {
        key.strip_prefix('c')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|ix| headers.get(ix).cloned())
    };

    let mut changed = Vec::new();
    if let Some(raw) = setting(&conn, COLUMN_SETTINGS_KEY)? {
        let map: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&raw).context("Parse column settings")?;
        let migrated: BTreeMap<String, serde_json::Value> = map
            .iter()
            .filter_map(|(key, value)| match name_of(key) {
                Some(name) => Some((name, value.clone())),
                // Already a name, or a column that no longer exists.
                None if headers.contains(key) => Some((key.clone(), value.clone())),
                None => None,
            })
            .collect();
        if migrated != map {
            backup(path)?;
            write_setting(
                &conn,
                COLUMN_SETTINGS_KEY,
                &serde_json::to_string(&migrated)?,
            )?;
            changed.push(format!(
                "{} of {} column settings re-keyed",
                migrated.len(),
                map.len()
            ));
        }
    }

    if let Some(raw) = setting(&conn, COLUMN_LAYOUT_KEY)? {
        let mut layout: serde_json::Value = serde_json::from_str(&raw).context("Parse layout")?;
        let keys: Vec<String> = serde_json::from_value(layout["keys"].clone()).unwrap_or_default();
        let migrated: Vec<String> = keys
            .iter()
            .map(|k| name_of(k).unwrap_or_else(|| k.clone()))
            .collect();
        if migrated != keys {
            backup(path)?;
            layout["keys"] = serde_json::to_value(&migrated)?;
            write_setting(&conn, COLUMN_LAYOUT_KEY, &serde_json::to_string(&layout)?)?;
            changed.push("column layout re-keyed".into());
        }
    }

    Ok(if changed.is_empty() {
        "already on column names".into()
    } else {
        changed.join(", ")
    })
}

/// `dataset_main`'s data columns, in physical order — `_row_id` is column 0 and not one of them.
fn headers(conn: &Connection) -> Result<Vec<String>> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'dataset_main'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    let stmt = conn.prepare("SELECT * FROM dataset_main")?;
    Ok(stmt
        .column_names()
        .into_iter()
        .filter(|name| !matches!(*name, "_row_id" | "_row_order"))
        .map(str::to_string)
        .collect())
}

fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM __settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
    .context("Read setting")
}

fn write_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO __settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .context("Write setting")?;
    Ok(())
}

/// Copy the file aside before the first write. Already having a backup means this ran before, and
/// overwriting it would destroy the only pre-migration copy.
fn backup(path: &Path) -> Result<()> {
    let backup = path.with_extension("qrate.bak");
    if backup.exists() {
        return Ok(());
    }
    std::fs::copy(path, &backup).context("Back up project")?;
    Ok(())
}
