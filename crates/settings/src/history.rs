//! The project's change log: every edit to the data and its notes, kept in the `.qrate` file for
//! good. Append-only — a restore is a new entry, never a rewind — so the log is an audit trail,
//! not an undo stack.
//!
//! Changes are keyed by stable [`RowId`] and column name rather than grid positions, which is what
//! lets an entry written a year ago still be read, and reversed, against today's grid. Positions
//! ride along only as the place to put a row or column back.
//!
//! Entries reach disk inside the same transaction as the data they describe (see
//! `project::save_dataset` and `project::write_notes`), so the log never claims a change the file
//! doesn't hold.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use gpui::App;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::project::RowId;

pub type EntryId = i64;

/// Where a change came from. The difference between a value an archivist typed and one they
/// accepted from a fix is the point of keeping a log at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Typed,
    Paste,
    Clear,
    ReplaceAll,
    Details,
    Spelling,
    /// A fix from the Fixes menu or the Problems panel, named by what offered it.
    Fix(String),
    /// Rows or columns added, removed, renamed, or moved.
    Structure,
    Undo,
    Redo,
    /// Put back by restoring the project, or a single value, to this entry.
    Restore(EntryId),
}

impl Origin {
    /// How the History panel names where a change came from.
    pub fn label(&self) -> String {
        match self {
            Origin::Typed => "Typed".into(),
            Origin::Paste => "Pasted".into(),
            Origin::Clear => "Cleared".into(),
            Origin::ReplaceAll => "Replace".into(),
            Origin::Details => "Details panel".into(),
            Origin::Spelling => "Spelling fix".into(),
            Origin::Fix(fix) => format!("Fix: {fix}"),
            Origin::Structure => "Rows and columns".into(),
            Origin::Undo => "Undo".into(),
            Origin::Redo => "Redo".into(),
            Origin::Restore(id) => format!("Restored to #{id}"),
        }
    }
}

/// One recorded change. Every variant is reversible from what it carries alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    Cell {
        row: RowId,
        column: String,
        before: String,
        after: String,
    },
    RowAdded {
        row: RowId,
        position: usize,
        cells: Vec<(String, String)>,
    },
    RowRemoved {
        row: RowId,
        position: usize,
        cells: Vec<(String, String)>,
    },
    ColumnAdded {
        column: String,
        position: usize,
        cells: Vec<(RowId, String)>,
    },
    ColumnRemoved {
        column: String,
        position: usize,
        cells: Vec<(RowId, String)>,
    },
    ColumnRenamed {
        before: String,
        after: String,
    },
    ColumnMoved {
        column: String,
        from: usize,
        to: usize,
    },
    /// `None` on either side is no note there. A row-less or column-less note widens to the whole
    /// column or row, as it does in `__notes`.
    Note {
        row: Option<RowId>,
        column: Option<String>,
        before: Option<String>,
        after: Option<String>,
    },
}

impl Change {
    /// The change that undoes this one.
    pub fn inverse(&self) -> Change {
        match self.clone() {
            Change::Cell {
                row,
                column,
                before,
                after,
            } => Change::Cell {
                row,
                column,
                before: after,
                after: before,
            },
            Change::RowAdded {
                row,
                position,
                cells,
            } => Change::RowRemoved {
                row,
                position,
                cells,
            },
            Change::RowRemoved {
                row,
                position,
                cells,
            } => Change::RowAdded {
                row,
                position,
                cells,
            },
            Change::ColumnAdded {
                column,
                position,
                cells,
            } => Change::ColumnRemoved {
                column,
                position,
                cells,
            },
            Change::ColumnRemoved {
                column,
                position,
                cells,
            } => Change::ColumnAdded {
                column,
                position,
                cells,
            },
            Change::ColumnRenamed { before, after } => Change::ColumnRenamed {
                before: after,
                after: before,
            },
            Change::ColumnMoved { column, from, to } => Change::ColumnMoved {
                column,
                from: to,
                to: from,
            },
            Change::Note {
                row,
                column,
                before,
                after,
            } => Change::Note {
                row,
                column,
                before: after,
                after: before,
            },
        }
    }

    /// The row and column this change is looked up by.
    fn key(&self) -> (Option<RowId>, Option<&str>) {
        match self {
            Change::Cell { row, column, .. } => (Some(*row), Some(column)),
            Change::RowAdded { row, .. } | Change::RowRemoved { row, .. } => (Some(*row), None),
            Change::ColumnAdded { column, .. }
            | Change::ColumnRemoved { column, .. }
            | Change::ColumnMoved { column, .. }
            | Change::ColumnRenamed { after: column, .. } => (None, Some(column)),
            Change::Note { row, column, .. } => (*row, column.as_deref()),
        }
    }
}

/// One user action: a paste, a restore, a single typed value — however many changes it made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `0` until the entry reaches disk.
    pub id: EntryId,
    /// Unix seconds, taken when the change was made rather than when it was saved.
    pub at: i64,
    pub author: Option<String>,
    pub origin: Origin,
    pub changes: Vec<Change>,
}

impl Entry {
    pub fn new(origin: Origin, changes: Vec<Change>, author: Option<String>) -> Self {
        Self {
            id: 0,
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
            author,
            origin,
            changes,
        }
    }
}

/// Who the archivist has told qrate they are, shared with notes. `None` when unset — the log
/// stays unsigned rather than guessing from the OS account.
pub fn author(cx: &App) -> Option<String> {
    if !cx.has_global::<crate::AppSettings>() {
        return None;
    }
    let author = crate::effective_text(crate::NOTE_AUTHOR_KEY, cx);
    (!author.trim().is_empty()).then(|| author.to_string())
}

const HISTORY_DDL: &str = r#"
    CREATE TABLE IF NOT EXISTS __history (
      id     INTEGER PRIMARY KEY,
      at     INTEGER NOT NULL,
      author TEXT,
      origin TEXT NOT NULL,
      label  TEXT
    );
    CREATE TABLE IF NOT EXISTS __history_changes (
      entry_id    INTEGER NOT NULL REFERENCES __history(id),
      seq         INTEGER NOT NULL,
      row_id      INTEGER,
      column_name TEXT,
      change      TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS __history_changes_cell ON __history_changes(row_id, column_name);
"#;

/// Write `entries` on `conn`, inside whatever transaction the caller holds.
pub(crate) fn append(conn: &Connection, entries: &[Entry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    conn.execute_batch(HISTORY_DDL)
        .context("Create history tables")?;
    let mut entry =
        conn.prepare("INSERT INTO __history(at, author, origin) VALUES (?1, ?2, ?3)")?;
    let mut change = conn.prepare(
        "INSERT INTO __history_changes(entry_id, seq, row_id, column_name, change)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for e in entries.iter().filter(|e| !e.changes.is_empty()) {
        entry
            .execute(params![e.at, e.author, serde_json::to_string(&e.origin)?])
            .context("Insert history entry")?;
        let id = conn.last_insert_rowid();
        for (seq, c) in e.changes.iter().enumerate() {
            let (row, column) = c.key();
            change
                .execute(params![id, seq, row, column, serde_json::to_string(c)?])
                .context("Insert history change")?;
        }
    }
    Ok(())
}

/// An entry as the History panel lists it: with its name, if it has been given one, and when it was
/// made in the archivist's own time zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub entry: Entry,
    pub label: Option<String>,
    /// `YYYY-MM-DD`, local.
    pub day: String,
    /// `HH:MM`, local.
    pub time: String,
    /// Whole days between `day` and today, so "Today" and "Yesterday" need no calendar here.
    pub days_ago: i64,
}

/// Every entry newer than `after`, oldest first. A file that has never logged anything reads as
/// an empty history.
pub fn entries_after(path: &Path, after: EntryId) -> Result<Vec<Entry>> {
    Ok(select(path, "id > ?1 ORDER BY id", after, -1)?
        .into_iter()
        .map(|listed| listed.entry)
        .collect())
}

/// Up to `limit` entries older than `before`, newest first — one page of the History panel.
pub fn page(path: &Path, before: EntryId, limit: i64) -> Result<Vec<Listed>> {
    select(path, "id < ?1 ORDER BY id DESC", before, limit)
}

fn select(path: &Path, filter: &str, bound: EntryId, limit: i64) -> Result<Vec<Listed>> {
    let conn = crate::project::open_ro(path)?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '__history'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    let local = "date(at, 'unixepoch', 'localtime')";
    let mut listed = conn
        .prepare(&format!(
            "SELECT id, at, author, origin, label, {local}, strftime('%H:%M', at, 'unixepoch', 'localtime'),
                    CAST(julianday(date('now', 'localtime')) - julianday({local}) AS INTEGER)
             FROM __history WHERE {filter} LIMIT ?2"
        ))?
        .query_map(params![bound, limit], |r| {
            Ok((
                (
                    r.get::<_, EntryId>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ),
                (
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ),
            ))
        })?
        .map(|row| {
            let ((id, at, author, origin), (label, day, time, days_ago)) = row?;
            Ok(Listed {
                entry: Entry {
                    id,
                    at,
                    author,
                    origin: serde_json::from_str(&origin).context("Read history origin")?,
                    changes: Vec::new(),
                },
                label,
                day,
                time,
                days_ago,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut changes =
        conn.prepare("SELECT change FROM __history_changes WHERE entry_id = ?1 ORDER BY seq")?;
    for listed in &mut listed {
        listed.entry.changes = changes
            .query_map([listed.entry.id], |r| r.get::<_, String>(0))?
            .map(|c| serde_json::from_str(&c?).context("Read history change"))
            .collect::<Result<_>>()?;
    }
    Ok(listed)
}

/// Name an entry — "Before Islandora ingest" — or, with `None`, take the name away again.
pub fn set_label(path: &Path, id: EntryId, label: Option<&str>) -> Result<()> {
    crate::project::open_rw(path)?
        .execute(
            "UPDATE __history SET label = ?2 WHERE id = ?1",
            params![id, label],
        )
        .context("Name history entry")?;
    Ok(())
}

/// Forget the whole log. The data it describes is untouched.
pub fn clear(path: &Path) -> Result<()> {
    let mut conn = crate::project::open_rw(path)?;
    let tx = conn.transaction()?;
    tx.execute_batch(HISTORY_DDL)?;
    tx.execute_batch("DELETE FROM __history_changes; DELETE FROM __history;")
        .context("Clear history")?;
    tx.commit().context("Commit cleared history")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Change, Entry, EntryId, Origin, clear, entries_after, page, set_label};
    use crate::project::{ProjectSpec, create_project_file, save_dataset, write_notes};

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("qrate-history-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        let headers = vec!["Title".to_string()];
        let rows = vec![vec!["one".to_string()]];
        create_project_file(
            &path,
            &ProjectSpec {
                name: "H",
                source: "CSV",
                headers: &headers,
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();
        path
    }

    fn edit(before: &str, after: &str) -> Change {
        Change::Cell {
            row: 1,
            column: "Title".into(),
            before: before.into(),
            after: after.into(),
        }
    }

    /// Entries written with the data and with the notes read back in order, whole, and numbered,
    /// and a file that has never logged anything reads as an empty history.
    #[test]
    fn entries_round_trip_through_both_writers() {
        let path = project("round-trip.qrate");
        assert_eq!(entries_after(&path, 0).unwrap(), Vec::new());

        let typed = Entry::new(Origin::Typed, vec![edit("one", "One")], Some("rk".into()));
        let fixed = Entry::new(
            Origin::Fix("Use “One.”".into()),
            vec![edit("One", "One.")],
            None,
        );
        let headers = vec!["Title".to_string()];
        save_dataset(
            &path,
            &headers,
            &[1],
            &[vec!["One.".into()]],
            &[typed.clone(), fixed.clone()],
        )
        .unwrap();
        let note = Entry::new(
            Origin::Clear,
            vec![Change::Note {
                row: Some(1),
                column: None,
                before: Some("faded".into()),
                after: None,
            }],
            None,
        );
        write_notes(&path, "note", &[], std::slice::from_ref(&note)).unwrap();

        let all = entries_after(&path, 0).unwrap();
        let ids: Vec<_> = all.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        let strip = |e: &Entry| Entry { id: 0, ..e.clone() };
        assert_eq!(
            all.iter().map(strip).collect::<Vec<_>>(),
            vec![typed, fixed, note]
        );
        assert_eq!(entries_after(&path, 2).unwrap().len(), 1);
    }

    /// The panel pages newest first, a name sticks to its entry until it is taken away, and
    /// clearing forgets the log without touching the data.
    #[test]
    fn pages_run_newest_first_and_names_and_clearing_stick() {
        let path = project("paging.qrate");
        let headers = vec!["Title".to_string()];
        let entries: Vec<Entry> = (0..5)
            .map(|n| {
                Entry::new(
                    Origin::Typed,
                    vec![edit(&n.to_string(), &(n + 1).to_string())],
                    None,
                )
            })
            .collect();
        save_dataset(&path, &headers, &[1], &[vec!["5".into()]], &entries).unwrap();

        let first = page(&path, EntryId::MAX, 2).unwrap();
        assert_eq!(
            first.iter().map(|l| l.entry.id).collect::<Vec<_>>(),
            vec![5, 4]
        );
        assert_eq!(first[0].days_ago, 0, "written just now");
        let older = page(&path, 4, 10).unwrap();
        assert_eq!(
            older.iter().map(|l| l.entry.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );

        set_label(&path, 2, Some("Before ingest")).unwrap();
        assert_eq!(
            page(&path, 3, 1).unwrap()[0].label.as_deref(),
            Some("Before ingest")
        );
        set_label(&path, 2, None).unwrap();
        assert_eq!(page(&path, 3, 1).unwrap()[0].label, None);

        clear(&path).unwrap();
        assert!(page(&path, EntryId::MAX, 10).unwrap().is_empty());
        let data = crate::project::load_project_file(&path).unwrap();
        assert_eq!(data.rows, vec![vec!["5".to_string()]]);
    }

    #[test]
    fn an_inverse_undoes_and_its_own_inverse_is_the_original() {
        let moved = Change::ColumnMoved {
            column: "Title".into(),
            from: 0,
            to: 3,
        };
        assert_eq!(edit("a", "b").inverse(), edit("b", "a"));
        assert_eq!(moved.inverse().inverse(), moved);
    }
}
