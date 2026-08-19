//! One-off migration from `.qrate` schema v2 to v3: preserve row identity independently from
//! display order and re-key authored notes from source positions to those stable ids.
//!
//! Deliberately a script rather than a startup migration: alpha builds have no installed-project
//! population to carry forever, and an explicit run can put a backup beside every file first.
//!
//! ```text
//! cargo run -p settings --example migrate_row_ids              # every recent project
//! cargo run -p settings --example migrate_row_ids -- a.qrate   # or named files
//! ```
//!
//! Writes a `.qrate.bak` beside each v2 file before changing it.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension as _, params};

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

fn recents() -> Result<Vec<String>> {
    let db = settings::data_dir()
        .context("No local data dir")?
        .join("settings.sqlite3");
    let conn = Connection::open(&db).context("Open settings database")?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT json FROM settings_kv WHERE key = 'app_settings_v1'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let blob: serde_json::Value = serde_json::from_str(raw.as_deref().unwrap_or("{}"))?;
    let list = blob["values"]["project_wizard.recent_projects"]["Text"]
        .as_str()
        .unwrap_or("[]");
    let entries: Vec<serde_json::Value> = serde_json::from_str(list).unwrap_or_default();
    Ok(entries
        .iter()
        .filter_map(|entry| entry.get("path")?.as_str().map(str::to_string))
        .collect())
}

fn migrate(path: &Path) -> Result<String> {
    let mut conn = Connection::open(path).context("Open project")?;
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 3 {
        return Ok("already schema v3".into());
    }
    if version != 2 {
        bail!("expected schema v2, found v{version}");
    }

    backup(path)?;
    let tx = conn.transaction()?;
    let dataset_exists = table_exists(&tx, "dataset_main")?;
    let notes_exist = table_exists(&tx, "__notes")?;
    let mut rows = 0;
    if dataset_exists {
        tx.execute(
            "ALTER TABLE dataset_main ADD COLUMN _row_order INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        let ids = {
            let mut stmt = tx.prepare("SELECT _row_id FROM dataset_main ORDER BY _row_id")?;
            stmt.query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        rows = ids.len();
        for (source, row_id) in ids.iter().copied().enumerate() {
            tx.execute(
                "UPDATE dataset_main SET _row_order = ?2 WHERE _row_id = ?1",
                params![row_id, source as i64],
            )?;
        }

        if notes_exist {
            tx.execute(
                "UPDATE __notes SET row_ix = -row_ix - 1
                 WHERE dataset = 'dataset_main' AND row_ix IS NOT NULL",
                [],
            )?;
            for (source, row_id) in ids.into_iter().enumerate() {
                tx.execute(
                    "UPDATE __notes SET row_ix = ?2
                     WHERE dataset = 'dataset_main' AND row_ix = ?1",
                    params![-(source as i64) - 1, row_id],
                )?;
            }
        }
    }
    tx.pragma_update(None, "user_version", 3)?;
    tx.commit().context("Migrate row ids")?;
    Ok(format!("migrated {rows} rows to schema v3"))
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn backup(path: &Path) -> Result<()> {
    let backup = path.with_extension("qrate.bak");
    if !backup.exists() {
        std::fs::copy(path, &backup).context("Back up project")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_v2_file_keeps_columns_rows_and_notes() {
        let dir = std::env::temp_dir().join("qrate-row-id-migration-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v2.qrate");
        let backup = path.with_extension("qrate.bak");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 2;
             CREATE TABLE __settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO __settings VALUES ('name', 'Legacy');
             CREATE TABLE __columns (name TEXT PRIMARY KEY, data_type TEXT NOT NULL, notes TEXT);
             CREATE TABLE dataset_main (_row_id INTEGER PRIMARY KEY, Title TEXT, Date TEXT);
             INSERT INTO dataset_main(Title, Date) VALUES ('First', '1901'), ('Second', '1902');
             CREATE TABLE __notes (
               dataset TEXT NOT NULL, row_ix INTEGER, column_name TEXT, severity TEXT NOT NULL,
               source TEXT NOT NULL, message TEXT NOT NULL, created_at TEXT, author TEXT
             );
             INSERT INTO __notes VALUES
               ('dataset_main', 1, 'Title', 'note', 'note', 'keep me', NULL, NULL);",
        )
        .unwrap();
        drop(conn);

        migrate(&path).unwrap();
        assert!(backup.exists());
        let data = settings::project::load_project_file(&path).unwrap();
        assert_eq!(data.headers, ["Title", "Date"]);
        assert_eq!(data.rows[1], ["Second", "1902"]);
        assert_eq!(data.row_ids, [1, 2]);
        let notes = settings::project::read_notes(&path).unwrap();
        assert_eq!((notes[0].row, notes[0].row_id), (Some(1), Some(2)));
    }
}
