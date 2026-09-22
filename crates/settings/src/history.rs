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

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone as _};
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

    /// The row and column a change concerns, which is how a reader asks whether it is about the
    /// item they are looking at without formatting it first.
    pub fn key(&self) -> (Option<RowId>, Option<&str>) {
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

/// Asks the History panel to show one cell's changes and come to the front — set by the grid's
/// "Show Edit History", which cannot reach the panel itself.
pub struct ShowCellHistory {
    pub row: RowId,
    pub column: String,
}

impl gpui::Global for ShowCellHistory {}

/// Who the archivist has told qrate they are, shared with notes. `None` when unset — the log
/// stays unsigned rather than guessing from the OS account.
pub fn author(cx: &App) -> Option<String> {
    if !cx.has_global::<crate::AppSettings>() {
        return None;
    }
    let author = crate::AppSettings::get(cx)
        .values
        .get(crate::NOTE_AUTHOR_KEY)
        .map(|value| value.text())
        .unwrap_or_default();
    (!author.trim().is_empty()).then(|| author.trim().to_string())
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
    CREATE INDEX IF NOT EXISTS __history_changes_entry ON __history_changes(entry_id, seq);
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
    Ok(select(path, "id > ?1 ORDER BY id", &[&after])?
        .into_iter()
        .map(|listed| listed.entry)
        .collect())
}

/// A unix timestamp as the History panel reads it: the local day, the local time, and how many
/// whole days back that day is from `today`.
///
/// `today` is handed in because a page converts hundreds of these, reading the clock is the
/// expensive half, and the answer only changes at midnight.
fn parts(at: i64, today: NaiveDate) -> (String, String, i64) {
    // An hour that a clock change repeated resolves to its first reading rather than refusing to
    // be a time at all; one that a clock change skipped has no local reading, so it is shown as
    // the UTC it was recorded as instead of not being shown.
    let when = match Local.timestamp_opt(at, 0).earliest() {
        Some(when) => when.naive_local(),
        None => DateTime::from_timestamp(at, 0)
            .unwrap_or_default()
            .naive_utc(),
    };
    (
        when.format("%Y-%m-%d").to_string(),
        when.format("%H:%M").to_string(),
        (today - when.date()).num_days(),
    )
}

/// Each of `ats` (unix seconds) as a local `(YYYY-MM-DD, HH:MM)`, for changes that have not
/// reached a file yet. Entries read out of a file are converted by [`select`] as they are read,
/// through the same [`parts`], so the two agree about what day a change was made on.
pub fn local_times(ats: &[i64]) -> Vec<(String, String)> {
    let today = Local::now().date_naive();
    ats.iter()
        .map(|at| {
            let (day, time, _) = parts(*at, today);
            (day, time)
        })
        .collect()
}

/// Up to `limit` entries older than `before`, newest first — one page of the History panel. With
/// `row`, only the entries that changed that row or a note on it.
pub fn page(path: &Path, before: EntryId, limit: i64, row: Option<RowId>) -> Result<Vec<Listed>> {
    select(
        path,
        "id < ?1 AND (?3 IS NULL OR id IN (SELECT entry_id FROM __history_changes WHERE row_id = ?3))
         ORDER BY id DESC LIMIT ?2",
        &[&before, &limit, &row],
    )
}

/// Every name `column` has gone by, newest first and starting with its own — so a cell's history
/// still finds the edits made before its column was renamed. `renames` is `(before, after)` pairs,
/// newest first.
pub fn former_names<'a>(
    column: &str,
    renames: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    let mut names = vec![column.to_string()];
    for (before, after) in renames {
        if names.last().is_some_and(|name| name == after) {
            names.push(before.to_string());
        }
    }
    names
}

fn select(path: &Path, filter: &str, bound: &[&dyn rusqlite::ToSql]) -> Result<Vec<Listed>> {
    let conn = crate::project::open_ro(path)?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '__history'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    // The clock is read once for the whole page rather than per row, which is what the three
    // `localtime` columns this query used to carry amounted to.
    let today = Local::now().date_naive();
    let mut listed = conn
        .prepare(&format!(
            "SELECT id, at, author, origin, label FROM __history WHERE {filter}"
        ))?
        .query_map(bound, |r| {
            Ok((
                r.get::<_, EntryId>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .map(|row| {
            let (id, at, author, origin, label) = row?;
            let (day, time, days_ago) = parts(at, today);
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
    // One query for the whole page's changes rather than one per entry. Per entry, each lookup
    // was a scan of `__history_changes` — a table that grows with the project, not with the page —
    // so reading 200 entries cost 200 scans of everything ever edited.
    //
    // The id list is interpolated because rusqlite binds no arrays without `rarray`; every value
    // is an `EntryId` this function just read out of the same table, so there is no text here to
    // escape.
    let mut changes: HashMap<EntryId, Vec<Change>> = HashMap::new();
    if !listed.is_empty() {
        let ids = listed
            .iter()
            .map(|listed| listed.entry.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT entry_id, change FROM __history_changes
             WHERE entry_id IN ({ids}) ORDER BY entry_id, seq"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, EntryId>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, change) = row?;
            changes
                .entry(id)
                .or_default()
                .push(serde_json::from_str(&change).context("Read history change")?);
        }
    }
    for listed in &mut listed {
        listed.entry.changes = changes.remove(&listed.entry.id).unwrap_or_default();
    }
    Ok(listed)
}

/// Every column rename in the log as `(before, after)`, newest first — what [`former_names`] follows.
pub fn renames(path: &Path) -> Result<Vec<(String, String)>> {
    let conn = crate::project::open_ro(path)?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '__history_changes'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    conn.prepare(
        "SELECT change FROM __history_changes WHERE row_id IS NULL AND change LIKE '{\"ColumnRenamed\"%'
         ORDER BY entry_id DESC, seq DESC",
    )?
    .query_map([], |r| r.get::<_, String>(0))?
    .filter_map(|change| match serde_json::from_str(&change.ok()?) {
        Ok(Change::ColumnRenamed { before, after }) => Some(Ok((before, after))),
        _ => None,
    })
    .collect()
}

/// How many entries a project keeps. Unset — or anything that isn't a positive number — keeps
/// every one, which is the default: this log is an audit trail, so nothing goes unless asked.
pub const HISTORY_LIMIT_KEY: &str = "history_limit";

/// What the Settings window offers for [`HISTORY_LIMIT_KEY`].
pub const HISTORY_LIMITS: &[(&str, &str)] = &[
    ("", "Keep everything (default)"),
    ("50000", "Keep the newest 50,000"),
    ("10000", "Keep the newest 10,000"),
    ("1000", "Keep the newest 1,000"),
];

/// The retention rule in force for the open project, or `None` to keep everything.
pub fn limit(cx: &App) -> Option<i64> {
    if !cx.has_global::<crate::AppSettings>() {
        return None;
    }
    crate::effective_text(HISTORY_LIMIT_KEY, cx)
        .parse::<i64>()
        .ok()
        .filter(|keep| *keep > 0)
}

/// Drop all but the newest `keep` entries, sparing every named version. Returns how many went.
///
/// A name is the archivist saying this entry matters, so a rule set to bound a file's size does
/// not quietly take those with it — which is also why the count a caller logs can be lower than
/// the number over the limit.
pub fn prune(path: &Path, keep: i64) -> Result<usize> {
    let conn = crate::project::open_rw(path)?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '__history'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }
    // Named entries are spared twice over: they are never in the doomed set, and they still count
    // towards the newest `keep`, so turning a rule on cannot cost more than the number it names.
    let doomed = "SELECT id FROM __history WHERE label IS NULL
                  AND id NOT IN (SELECT id FROM __history ORDER BY id DESC LIMIT ?1)";
    conn.execute(
        &format!("DELETE FROM __history_changes WHERE entry_id IN ({doomed})"),
        [keep],
    )
    .context("Prune history changes")?;
    let gone = conn
        .execute(
            &format!("DELETE FROM __history WHERE id IN ({doomed})"),
            [keep],
        )
        .context("Prune history entries")?;
    Ok(gone)
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
    use super::{
        Change, Entry, EntryId, Origin, clear, entries_after, former_names, local_times, page,
        prune, renames, set_label,
    };
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
            None,
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
        save_dataset(&path, &headers, &[1], &[vec!["5".into()]], None, &entries).unwrap();

        let first = page(&path, EntryId::MAX, 2, None).unwrap();
        let (day, time) = local_times(&[first[0].entry.at]).remove(0);
        assert_eq!(
            (day.as_str(), time.as_str()),
            (first[0].day.as_str(), first[0].time.as_str())
        );
        assert_eq!(
            first.iter().map(|l| l.entry.id).collect::<Vec<_>>(),
            vec![5, 4]
        );
        assert_eq!(first[0].days_ago, 0, "written just now");
        let older = page(&path, 4, 10, None).unwrap();
        assert_eq!(
            older.iter().map(|l| l.entry.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );

        set_label(&path, 2, Some("Before ingest")).unwrap();
        assert_eq!(
            page(&path, 3, 1, None).unwrap()[0].label.as_deref(),
            Some("Before ingest")
        );
        set_label(&path, 2, None).unwrap();
        assert_eq!(page(&path, 3, 1, None).unwrap()[0].label, None);

        clear(&path).unwrap();
        assert!(page(&path, EntryId::MAX, 10, None).unwrap().is_empty());
        let data = crate::project::load_project_file(&path).unwrap();
        assert_eq!(data.rows, vec![vec!["5".to_string()]]);
    }

    /// A row's history is the entries that touched it, and a cell's follows its column back
    /// through every rename.
    #[test]
    fn a_row_page_and_a_renamed_column_find_their_older_edits() {
        let path = project("row.qrate");
        let headers = vec!["Title".to_string()];
        let other = Change::Cell {
            row: 2,
            column: "Name".into(),
            before: String::new(),
            after: "x".into(),
        };
        let entries = vec![
            Entry::new(Origin::Typed, vec![edit("a", "b")], None),
            Entry::new(Origin::Typed, vec![other], None),
            Entry::new(
                Origin::Structure,
                vec![Change::ColumnRenamed {
                    before: "Name".into(),
                    after: "Title".into(),
                }],
                None,
            ),
        ];
        save_dataset(&path, &headers, &[1], &[vec!["b".into()]], None, &entries).unwrap();

        let row = page(&path, EntryId::MAX, 10, Some(1)).unwrap();
        assert_eq!(row.iter().map(|l| l.entry.id).collect::<Vec<_>>(), vec![1]);

        let renamed = renames(&path).unwrap();
        assert_eq!(renamed, vec![("Name".to_string(), "Title".to_string())]);
        let pairs = renamed.iter().map(|(b, a)| (b.as_str(), a.as_str()));
        assert_eq!(former_names("Title", pairs), vec!["Title", "Name"]);
        assert_eq!(former_names("Other", [("Name", "Title")]), vec!["Other"]);
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

    /// The History panel splits a day on `-` to name the month and prints the time straight
    /// through, so the shape these come back in is load-bearing even though the values are
    /// whatever the machine's own clock and zone say. SQLite used to guarantee it; now that both
    /// a saved entry and an unsaved one are converted by the same Rust, nothing else would catch
    /// the format drifting.
    #[test]
    fn a_local_timestamp_keeps_the_shape_the_panel_parses() {
        let (day, time) = local_times(&[1_700_000_000]).remove(0);
        assert_eq!(day.len(), 10, "expected YYYY-MM-DD, got {day}");
        let fields: Vec<&str> = day.split('-').collect();
        assert_eq!(fields.len(), 3, "expected YYYY-MM-DD, got {day}");
        assert!(
            fields.iter().all(|f| f.chars().all(|c| c.is_ascii_digit())),
            "expected YYYY-MM-DD, got {day}"
        );
        assert_eq!(time.len(), 5, "expected HH:MM, got {time}");
        assert_eq!(&time[2..3], ":", "expected HH:MM, got {time}");
    }

    /// Retention keeps the newest entries and spares every named one, however old. A name is the
    /// archivist marking that entry as the one to come back to, so a rule set to bound a file's
    /// size must not be what takes it away — and the changes of a dropped entry go with it rather
    /// than being left behind pointing at an id that no longer exists.
    #[test]
    fn pruning_keeps_the_newest_and_never_a_named_version() {
        let path = project("prune.qrate");
        let headers = vec!["Title".to_string()];
        let log: Vec<Entry> = (0..6)
            .map(|n| {
                Entry::new(
                    Origin::Typed,
                    vec![edit(&n.to_string(), &(n + 1).to_string())],
                    None,
                )
            })
            .collect();
        save_dataset(&path, &headers, &[1], &[vec!["6".into()]], None, &log).unwrap();

        // The oldest entry of the six, named — the one a retention rule would otherwise reach first.
        set_label(&path, 1, Some("Before ingest")).unwrap();

        let gone = prune(&path, 2).unwrap();
        assert_eq!(gone, 3, "six, less the newest two, less the named one");

        let kept = page(&path, EntryId::MAX, 10, None).unwrap();
        let ids: Vec<_> = kept.iter().map(|l| l.entry.id).collect();
        assert_eq!(
            ids,
            vec![6, 5, 1],
            "the newest two, and the named one below them"
        );
        assert_eq!(
            kept[2].label.as_deref(),
            Some("Before ingest"),
            "kept because it is named, not because it is recent"
        );
        // Every surviving entry still has its own changes, and no dropped entry left any behind.
        assert!(
            kept.iter().all(|l| l.entry.changes.len() == 1),
            "a pruned entry takes its changes with it"
        );

        // Nothing over the limit left to drop.
        assert_eq!(prune(&path, 2).unwrap(), 0);
    }

    /// A page fetches every entry's changes in one query and hands each entry its own back. Two
    /// entries, each with several changes, is the arrangement that catches a distribution that
    /// mixes them up or loses the order they were written in — which a restore reads as the wrong
    /// value to put back.
    #[test]
    fn a_page_keeps_each_entrys_changes_together_and_in_order() {
        let path = project("change-order.qrate");
        let headers = vec!["Title".to_string()];
        let first = Entry::new(
            Origin::Paste,
            vec![edit("a", "b"), edit("b", "c"), edit("c", "d")],
            None,
        );
        let second = Entry::new(Origin::Typed, vec![edit("d", "e"), edit("e", "f")], None);
        save_dataset(
            &path,
            &headers,
            &[1],
            &[vec!["f".into()]],
            None,
            &[first.clone(), second.clone()],
        )
        .unwrap();

        let listed = page(&path, EntryId::MAX, 10, None).unwrap();
        let changes: Vec<_> = listed
            .iter()
            .map(|listed| listed.entry.changes.clone())
            .collect();
        // Newest first, so the second entry leads.
        assert_eq!(changes, vec![second.changes, first.changes]);
    }
}
