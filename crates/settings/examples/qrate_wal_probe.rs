//! Standalone probe for ASNT-16: does a `.qrate` SQLite file in WAL mode stay
//! a single file across app open/close cycles, or does it leave `-wal`/`-shm`
//! siblings behind? Each invocation of this binary is one simulated
//! "app open -> write -> app close" cycle (a real process start/exit, not an
//! in-process loop) so the close behavior we observe is the real one, not an
//! artifact of staying in the same process.
//!
//! Writes to a scratch dir under the OS temp dir, never the repo.
//!
//! Usage: cargo run -p settings --example qrate_wal_probe -- <mode>
//!   write       open in WAL mode, write rows, checkpoint(TRUNCATE), close  [default]
//!   write-raw   open in WAL mode, write rows, close with NO checkpoint
//!   delete-mode open, switch journal_mode back to DELETE, close
//!   inspect     just list the scratch dir, no db writes
//!   reset       delete the scratch dir and start clean
//!   hang        write, then block forever (for external kill -9 testing)
//!   two-conn    open a 2nd connection alongside the writer; close them one
//!               at a time to see whether cleanup needs ALL handles gone

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::Connection;

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join("qrate-wal-probe")
}

fn db_path() -> PathBuf {
    scratch_dir().join("probe.qrate")
}

fn list_dir(dir: &Path, label: &str) {
    println!("--- {label} ---");
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    if entries.is_empty() {
        println!("  (empty)");
    }
    for entry in entries {
        let meta = entry.metadata().ok();
        let size = meta.map(|m| m.len()).unwrap_or(0);
        println!(
            "  {:<24} {} bytes",
            entry.file_name().to_string_lossy(),
            size
        );
    }
}

fn ensure_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS probe_kv (key TEXT PRIMARY KEY, val TEXT NOT NULL);",
    )
    .expect("create schema");
}

fn write_rows(conn: &Connection) {
    for i in 0..20 {
        conn.execute(
            "INSERT INTO probe_kv(key, val) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET val = excluded.val",
            rusqlite::params![format!("k{i}"), format!("run-{}", std::process::id())],
        )
        .expect("insert row");
    }
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "write".to_string());
    let dir = scratch_dir();

    if mode == "reset" {
        let _ = fs::remove_dir_all(&dir);
        println!("scratch dir reset: {dir:?}");
        return;
    }

    fs::create_dir_all(&dir).expect("create scratch dir");
    let path = db_path();

    if mode == "inspect" {
        list_dir(&dir, &format!("inspect ({})", path.display()));
        return;
    }

    if mode == "two-conn" {
        let writer = Connection::open(&path).expect("open writer");
        writer
            .pragma_update(None, "journal_mode", "WAL")
            .expect("set WAL");
        ensure_schema(&writer);
        write_rows(&writer);
        let reader = Connection::open(&path).expect("open reader");
        list_dir(&dir, "two connections open (writer + reader)");

        drop(reader);
        list_dir(&dir, "reader closed, writer STILL open");

        drop(writer);
        list_dir(&dir, "writer also closed (all handles gone)");
        return;
    }

    println!(
        "pid {} | mode {mode} | db {}",
        std::process::id(),
        path.display()
    );
    list_dir(&dir, "before open");

    let conn = Connection::open(&path).expect("open db");
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("set WAL");
    ensure_schema(&conn);

    write_rows(&conn);
    list_dir(&dir, "after writes, before close");

    match mode.as_str() {
        "write" => {
            conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
                .expect("checkpoint TRUNCATE");
            list_dir(&dir, "after checkpoint(TRUNCATE), before drop");
        }
        "delete-mode" => {
            conn.pragma_update(None, "journal_mode", "DELETE")
                .expect("switch back to DELETE");
            list_dir(&dir, "after journal_mode=DELETE, before drop");
        }
        "write-raw" => {
            // No checkpoint, no mode switch: just see what a plain close leaves behind.
        }
        "hang" => {
            println!("hanging with connection open; kill -9 this pid to simulate a crash");
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        other => eprintln!("unknown mode {other:?}, treating as write-raw"),
    }

    drop(conn);
    list_dir(&dir, "after connection dropped (process about to exit)");
}
