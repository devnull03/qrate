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
//!   pragma-syntax  write, then checkpoint with `PRAGMA wal_checkpoint =
//!               TRUNCATE` (assignment form, what rusqlite's pragma_update
//!               emits) vs `PRAGMA wal_checkpoint(TRUNCATE)` (function-call
//!               form) on freshly-written data each time, comparing the
//!               returned (busy, log, checkpointed) rows

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

// `nonce` makes repeated calls within one process write genuinely distinct
// bytes (a fixed pid-derived value would let SQLite's page cache produce a
// byte-identical page across calls, which is not representative of real
// usage and made an earlier version of this probe's checkpoint counts look
// wrong).
fn write_rows(conn: &Connection, nonce: u32) {
    for i in 0..20 {
        conn.execute(
            "INSERT INTO probe_kv(key, val) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET val = excluded.val",
            rusqlite::params![
                format!("k{i}"),
                format!("run-{}-{nonce}", std::process::id())
            ],
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

    if mode == "pragma-syntax" {
        let conn = Connection::open(&path).expect("open db");
        conn.pragma_update(None, "journal_mode", "WAL")
            .expect("set WAL");
        ensure_schema(&conn);

        write_rows(&conn, 1);
        list_dir(&dir, "after writes, before function-call-form checkpoint");

        // Isolate: rusqlite's dedicated pragma() method (takes a value +
        // row callback) vs raw SQL text through query_row, both TRUNCATE.
        let mut via_pragma_method = (-1i64, -1i64, -1i64);
        conn.pragma(None, "wal_checkpoint", "TRUNCATE", |r| {
            via_pragma_method = (r.get(0)?, r.get(1)?, r.get(2)?);
            Ok(())
        })
        .expect("pragma() method form");
        println!(
            "via conn.pragma() method, TRUNCATE: busy={} log={} checkpointed={}",
            via_pragma_method.0, via_pragma_method.1, via_pragma_method.2
        );
        list_dir(&dir, "after conn.pragma() method TRUNCATE checkpoint");

        write_rows(&conn, 2);
        let (busy, log, checkpointed): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .expect("function-call form");
        println!(
            "function-call form  PRAGMA wal_checkpoint(TRUNCATE): busy={busy} log={log} checkpointed={checkpointed}"
        );
        list_dir(&dir, "after function-call form checkpoint");

        write_rows(&conn, 3);
        let (busy, log, checkpointed): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint = TRUNCATE", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .expect("assignment form, raw SQL");
        println!(
            "assignment form (raw SQL) PRAGMA wal_checkpoint = TRUNCATE: busy={busy} log={log} checkpointed={checkpointed}"
        );
        list_dir(&dir, "after assignment-form (raw SQL) checkpoint");

        write_rows(&conn, 4);
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .expect("assignment form via rusqlite pragma_update");
        // pragma_update() discards the returned row, so re-query via a
        // no-op PASSIVE checkpoint immediately after: if the prior
        // TRUNCATE actually ran, there is nothing left to checkpoint and
        // this reports checkpointed=0/log=0.
        let (busy, log, checkpointed): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .expect("passive follow-up checkpoint");
        println!(
            "after rusqlite pragma_update() assignment form, follow-up PASSIVE checkpoint: busy={busy} log={log} checkpointed={checkpointed} (0/0/0 confirms the prior TRUNCATE via pragma_update actually ran)"
        );
        list_dir(
            &dir,
            "after rusqlite pragma_update() assignment-form checkpoint",
        );

        // Ground truth: the (busy, log, checkpointed) row is secondary —
        // what actually matters is whether the data survived. Check row
        // count + values on the same connection, then again from a brand
        // new connection (proving it's durably in the main db file, not
        // just cached), to settle whether TRUNCATE's 0/0/0 report above
        // means "nothing happened" or just "nothing left to report".
        let (count, sample): (i64, String) = conn
            .query_row("SELECT count(*), max(val) FROM probe_kv", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("verify data on same connection");
        println!("same-connection verify: {count} rows, sample val = {sample:?}");
        drop(conn);

        let fresh = Connection::open(&path).expect("reopen with fresh connection");
        let (count, sample): (i64, String) = fresh
            .query_row("SELECT count(*), max(val) FROM probe_kv", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("verify data on fresh connection");
        println!("fresh-connection verify: {count} rows, sample val = {sample:?}");
        assert_eq!(count, 20, "row count did not survive the checkpoint cycles");
        return;
    }

    if mode == "two-conn" {
        let writer = Connection::open(&path).expect("open writer");
        writer
            .pragma_update(None, "journal_mode", "WAL")
            .expect("set WAL");
        ensure_schema(&writer);
        write_rows(&writer, 1);
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

    write_rows(&conn, 1);
    list_dir(&dir, "after writes, before close");

    match mode.as_str() {
        "write" => {
            // rusqlite's pragma_update() emits the assignment form
            // (`PRAGMA wal_checkpoint = TRUNCATE`). SQLite's grammar treats
            // `PRAGMA name = value` and `PRAGMA name(value)` as the same
            // parse (see PRAGMA docs: "value" and "(value)" are
            // interchangeable), confirmed empirically in `pragma-syntax`
            // mode: both forms, plus rusqlite's pragma()/pragma_update()
            // wrappers, produce identical file-level effects (WAL
            // truncated to 0 bytes, data durably present on a fresh
            // connection). Note: TRUNCATE mode's own (busy, log,
            // checkpointed) return row reports 0/0/0 regardless of how
            // many frames existed or which syntax is used - that's a
            // SQLite reporting quirk specific to TRUNCATE mode (a bare/
            // PASSIVE checkpoint correctly reports real frame counts), not
            // a sign the checkpoint didn't run. So the real check here is
            // the WAL file's on-disk size, not the returned row.
            let (busy, log, checkpointed): (i64, i64, i64) = conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .expect("checkpoint TRUNCATE (function-call form)");
            println!(
                "checkpoint(TRUNCATE) result: busy={busy} log={log} checkpointed={checkpointed}"
            );
            assert_eq!(busy, 0, "checkpoint was busy/blocked");
            list_dir(&dir, "after checkpoint(TRUNCATE), before drop");
            let wal_bytes = fs::metadata(format!("{}-wal", path.display()))
                .map(|m| m.len())
                .unwrap_or(0);
            assert_eq!(wal_bytes, 0, "WAL file was not truncated to 0 bytes");
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
