//! The table view: a virtualized data grid with a pinned row-number column, native
//! cell/row/column selection, movable + resizable columns (layout persisted per project), and
//! double-click-to-edit cells.
//!
//! This crate owns all table *state* but no dock chrome and no notion of other views —
//! `TablePanel` is a plain `Render` view that `workspace` mounts inside its own view host, which
//! is where the view switcher and any non-grid view live. Cross-crate readers (`workspace`'s
//! Details panel, `app`'s status-bar items) reach the shared state through `TableStateHandle`.

mod agent;
mod cell;
mod delegate;
mod editing;
pub mod editor;
pub mod file_links;
mod filter;
pub mod floating;
mod hierarchy;
mod history;
mod note;
mod panel;
pub mod photos;
mod relink;
mod row_index;
mod visual;

pub use agent::{AGENT_SOURCE, respond_to_agent, respond_to_agent_async};
pub use delegate::{QrateTableDelegate, Selection, TableChanged};
pub use editor::editor_box;
/// The grid's right-click menu, so a gallery card can raise the same one a row does rather than
/// growing a second, quietly diverging copy.
pub use note::{Target as MenuTarget, menu as context_menu};
pub use panel::watch::NewFiles;
pub use panel::{
    Clear, CollapseAll, Copy, Cut, DeleteColumn, DeleteRow, DeleteSubtree, Deselect, DuplicateRow,
    EditCell, ExpandAll, GRID_CONTEXT, ImportFiles, ImportSpreadsheet, IndentRow, InsertColumnLeft,
    InsertColumnRight, InsertNote, InsertRowAbove, InsertRowBelow, OutdentRow, Paste, Redo,
    RelinkMissingFiles, RenameColumn, Replace, Search, TablePanel, Undo, UnfreezeColumns,
    register_global_actions,
};
pub use visual::remove_model as remove_visual_model;

/// Global command handle for import entry points outside the centre table, such as Details.
pub struct TablePanelHandle(pub WeakEntity<TablePanel>);
impl Global for TablePanelHandle {}

/// Settings key (in either scope) for the alternating-row-stripe toggle.
pub const TABLE_STRIPES_KEY: &str = "table_stripes";

/// Settings key (either scope) for how tall grid rows are: comfortable (unset) or `compact`.
pub const ROW_DENSITY_KEY: &str = "table_row_density";

/// What Settings offers for [`ROW_DENSITY_KEY`].
pub const ROW_DENSITIES: &[(&str, &str)] = &[("", "Comfortable (default)"), ("compact", "Compact")];

/// Settings key (either scope) for how many lines of text every grid row holds: 1 (unset) to 4.
pub const ROW_LINES_KEY: &str = "table_row_lines";

/// What Settings offers for [`ROW_LINES_KEY`].
pub const ROW_LINES: &[(&str, &str)] = &[
    ("", "1 line (default)"),
    ("2", "2 lines"),
    ("3", "3 lines"),
    ("4", "4 lines"),
];

pub(crate) const MAX_ROW_LINES: usize = 4;

/// The row height in force, in lines, clamped to what Settings offers.
pub(crate) fn row_lines(cx: &App) -> usize {
    settings::effective_text(ROW_LINES_KEY, cx)
        .parse::<usize>()
        .map_or(1, |lines| lines.clamp(1, MAX_ROW_LINES))
}

/// Write the row height where it takes effect: the project's own value if it overrides the
/// default, the user default otherwise. One line is stored as unset, like the Settings default.
pub(crate) fn set_row_lines(lines: usize, cx: &mut App) {
    let value = match lines.clamp(1, MAX_ROW_LINES) {
        1 => SharedString::default(),
        lines => lines.to_string().into(),
    };
    if settings::has_project_override(ROW_LINES_KEY, cx) {
        settings::project::CurrentProject::set_text(ROW_LINES_KEY, value, cx);
    } else {
        settings::AppSettings::set_text(ROW_LINES_KEY, value, cx);
    }
}

/// Settings key (either scope) for how many edits Undo can step back through.
pub const UNDO_STEPS_KEY: &str = "undo_steps";

/// What Settings offers for [`UNDO_STEPS_KEY`]. The last is a ceiling: every step keeps what it
/// replaced, so a deep stack of whole-column pastes is real memory.
pub const UNDO_STEPS: &[(&str, &str)] = &[
    ("", "200 (default)"),
    ("500", "500"),
    ("1000", "1,000"),
    ("2000", "2,000"),
];

pub(crate) const DEFAULT_UNDO_STEPS: usize = 200;
const MAX_UNDO_STEPS: usize = 2000;

/// The undo depth in force, clamped so a hand-edited value cannot make the stack unbounded.
pub(crate) fn undo_steps(cx: &App) -> usize {
    settings::effective_text(UNDO_STEPS_KEY, cx)
        .parse::<usize>()
        .map_or(DEFAULT_UNDO_STEPS, |steps| steps.clamp(1, MAX_UNDO_STEPS))
}

use gpui::{
    App, Bounds, ClipboardItem, Entity, Global, Pixels, Point, PromptLevel, SharedString,
    WeakEntity, px, size,
};
use gpui_component::table::TableState;
use plugin_api::CommandContext;
use settings::columns::ColumnType;
use settings::history::{Change, EntryId, Origin};

/// Global handle to the live table state, so cross-crate status-bar items (the fake-data button
/// and the selected-cell widget in the `app` crate) can reach the table.
pub struct TableStateHandle(pub WeakEntity<TableState<QrateTableDelegate>>);
impl Global for TableStateHandle {}

/// The table area's window-space rectangle, checked each frame and published when it changes (see
/// `panel.rs`). It's both the
/// origin the floating cell editor is positioned against and the limit it grows to, so a long
/// value wraps within the panel instead of spilling over a side panel. The note editor clamps to
/// it too, via `clamped_float`.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TableViewportBounds(pub Bounds<Pixels>);
impl Global for TableViewportBounds {}

impl Default for TableViewportBounds {
    /// A rect large enough to be a no-op clamp until the first real measurement lands (editing
    /// can't start before the panel has rendered once anyway).
    fn default() -> Self {
        Self(Bounds::new(Default::default(), size(px(4000.), px(4000.))))
    }
}

/// Where the in-progress edit opened: what it is editing, its full window-space rect, and the
/// table's scroll offset at that instant. Written once per edit by `cell.rs` (or `filter.rs` for a
/// header rename — whatever is being edited is on screen when the edit starts), read by `panel.rs`
/// to place and size the floating editor, which is why the box stays put when the grid scrolls out
/// from under it, Sheets-style. `scroll` is filled in on the next panel render, since the scroll
/// handles live on `TableState` and aren't reachable from a cell. Cleared by `editing::start` so
/// re-editing the same cell re-measures.
pub(crate) struct EditSpawn {
    pub at: editing::EditState,
    pub bounds: Bounds<Pixels>,
    pub scroll: Option<Point<Pixels>>,
}
impl Global for EditSpawn {}

/// Re-run every registered validator against the live table. The other half of reloading plugins:
/// dropping one clears its findings, but only a run publishes the replacements.
#[track_caller]
pub fn revalidate_now(cx: &mut App) {
    log::debug!("revalidate requested at {}", std::panic::Location::caller());
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    state.update(cx, |state, cx| state.delegate().revalidate(cx));
}

/// Save in the background unless autosave is switched off. What `TablePanel`'s debounce ends in,
/// and what the paths that act on an explicitly clicked target call straight away — a menu click
/// is one deliberate change, not a burst to coalesce.
///
/// The grid is snapshotted here, on the UI thread, and written off it. A failure keeps the project
/// dirty and is put in front of the archivist, once per run of failures.
pub(crate) fn autosave(cx: &mut App) {
    stamp_pending_author(cx);
    if settings::effective_text(settings::AUTOSAVE_KEY, cx).as_ref() == "off" {
        return;
    }
    let Some((file, state)) = save_target(cx) else {
        return;
    };
    let snapshot = state.read(cx).delegate().save_snapshot();
    let edits = snapshot.edits;
    cx.spawn(async move |cx| {
        let written = cx
            .background_executor()
            .spawn({
                let file = file.clone();
                async move { write_snapshot(&file, &snapshot) }
            })
            .await;
        cx.update(|cx| {
            if let Err(message) = saved(&state, &file, edits, written, cx) {
                complain(message, cx);
            }
        });
    })
    .detach();
}

/// The open project's file and live table, which is what a save needs.
fn save_target(cx: &App) -> Option<(std::path::PathBuf, Entity<TableState<QrateTableDelegate>>)> {
    let file = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|p| p.file.clone())?;
    let state = cx.try_global::<TableStateHandle>()?.0.upgrade()?;
    Some((file, state))
}

/// Write a snapshot, taking its turn behind any other save of the same dataset. Safe off the UI
/// thread. The error is the reason alone, for the caller to put into a sentence.
fn write_snapshot(file: &std::path::Path, snapshot: &delegate::SaveSnapshot) -> Result<(), String> {
    use std::sync::atomic::Ordering::SeqCst;

    let ledger = &snapshot.ledger;
    let _turn = ledger
        .write
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ledger.edits.load(SeqCst) > snapshot.edits {
        return Ok(());
    }
    let skip = (ledger.written.load(SeqCst).saturating_sub(snapshot.first) as usize)
        .min(snapshot.history.len());
    settings::project::save_changes(
        file,
        &snapshot.headers,
        &snapshot.row_ids,
        &snapshot.rows,
        Some(&snapshot.structure),
        &snapshot.history[skip..],
    )
    .map_err(|err| format!("{err:#}"))?;
    ledger
        .written
        .store(snapshot.first + snapshot.history.len() as u64, SeqCst);
    ledger.edits.store(snapshot.edits, SeqCst);
    Ok(())
}

/// Whether the archivist has been told autosave is failing, so a run of failures prompts once.
struct SaveTrouble(bool);
impl Global for SaveTrouble {}

/// Everything after a save lands: the written log entries leave the delegate, and the project
/// stops being dirty if nothing changed while it wrote. The error names the file for the archivist.
fn saved(
    state: &Entity<TableState<QrateTableDelegate>>,
    file: &std::path::Path,
    edits: u64,
    written: Result<(), String>,
    cx: &mut App,
) -> Result<(), String> {
    let settled = state.update(cx, |state, _| {
        let delegate = state.delegate_mut();
        delegate.history_written();
        delegate.edits() == edits
    });
    if let Err(err) = written {
        let message = format!("Couldn't save {}: {err}", file.display());
        log::error!("{message}");
        return Err(message);
    }
    cx.set_global(SaveTrouble(false));
    let current = cx
        .try_global::<settings::project::CurrentProject>()
        .is_some_and(|project| project.file == file);
    if settled && current {
        settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
    }
    Ok(())
}

/// Put a failed autosave in front of the archivist, over whichever window is active.
fn complain(message: String, cx: &mut App) {
    if cx.try_global::<SaveTrouble>().is_some_and(|told| told.0) {
        return;
    }
    cx.set_global(SaveTrouble(true));
    let Some(window) = cx.active_window() else {
        return;
    };
    let detail = format!(
        "{message}\n\nThey are still open here. Autosave keeps trying, or save with Ctrl+S."
    );
    window
        .update(cx, |_, window, cx| {
            drop(window.prompt(
                PromptLevel::Critical,
                "Your changes were not saved",
                Some(&detail),
                &["OK"],
                cx,
            ))
        })
        .ok();
}

pub(crate) fn stamp_pending_author(cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    let author = settings::history::author(cx);
    state.update(cx, |state, _| {
        state.delegate_mut().stamp_pending_author(author)
    });
}

/// Commit `text` into a cell, mark the project dirty, and re-run validation — everything a
/// committed edit does except going through the inline editor. What a fix menu applies through.
pub fn write_cell(row: usize, col: usize, text: SharedString, origin: Origin, cx: &mut App) {
    write_cells(vec![(row, col, text)], origin, cx);
}

/// The selection as tab-separated text on the clipboard — what Ctrl+C copies, reachable from
/// outside the grid so Details' "Copy N rows" puts the same thing there as the keystroke does.
pub fn copy_selection(cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let delegate = state.read(cx).delegate();
    let Some((rows, cols)) = delegate.range_cells() else {
        return;
    };
    let lines: Vec<String> = rows
        .iter()
        .map(|&row| {
            cols.clone()
                .map(|col| delegate.cell(row, col).map_or("", |v| v.as_ref()))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
}

/// Drop a multi-selection back to nothing — Details' "Clear". The cursor goes with it, so the
/// panel falls to its empty state rather than silently keeping one row of the bundle.
pub fn clear_selection(cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    state.update(cx, |state, cx| {
        state.delegate_mut().clear_selection();
        cx.emit(delegate::TableChanged);
        cx.notify();
    });
}

/// [`write_cell`]'s bulk form: one undo step for the whole batch, one validation pass at the end,
/// one save. For menu items, which act on an explicitly clicked target; the keyboard and clipboard
/// paths go through `TablePanel`'s own method of the same name, which debounces both instead.
pub fn write_cells(cells: Vec<(usize, usize, SharedString)>, origin: Origin, cx: &mut App) {
    if cells.is_empty() {
        return;
    }
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    state.update(cx, |state, cx| {
        let files = file_rows(state.delegate(), &cells);
        state.delegate_mut().apply_edit(cells, origin);
        if let Some(rows) = files {
            photos::refresh(state, Some(&rows), cx);
        }
        cx.emit(delegate::TableChanged);
        cx.notify();
    });
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
    autosave(cx);
}

/// The rows a batch of edits renames a file in, when it touches a column the project declares as
/// holding a filename — the only edits that can change which file a row previews.
pub(crate) fn file_rows(
    delegate: &delegate::QrateTableDelegate,
    cells: &[(usize, usize, SharedString)],
) -> Option<Vec<usize>> {
    cells
        .iter()
        .any(|(_, col, _)| delegate.column_type(*col) == ColumnType::Filename)
        .then(|| cells.iter().map(|(row, _, _)| *row).collect())
}

/// The rows `changes` put new text in or back into, as they now sit; `None` when a column moved,
/// which can change what every row resolves to.
fn changed_rows(delegate: &QrateTableDelegate, changes: &[Change]) -> Option<Vec<usize>> {
    let position: std::collections::HashMap<_, _> = delegate
        .row_ids()
        .iter()
        .enumerate()
        .map(|(at, id)| (*id, at))
        .collect();
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Cell { row, .. } | Change::RowAdded { row, .. } => {
                Some(position.get(row).copied())
            }
            Change::RowRemoved { .. } | Change::Note { .. } => None,
            _ => Some(None),
        })
        .collect()
}

/// Undo or redo the last grid edit, then do everything a committed edit does. A free function
/// rather than a `TablePanel` method because the panels that edit the grid from outside it — and
/// the Edit menu, which dispatches wherever focus happens to sit — can't reach the grid's view.
/// Nothing happens on an empty stack.
pub fn history_step(redo: bool, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let changes = state.update(cx, |state, _| match redo {
        true => state.delegate_mut().redo(),
        false => state.delegate_mut().undo(),
    });
    let Some(changes) = changes else {
        return;
    };
    let origin = match redo {
        true => Origin::Redo,
        false => Origin::Undo,
    };
    settle(&state, &changes, origin, cx);
}

/// Everything that follows the grid changing under the user rather than one cell at a time — an
/// undo, a redo, a restore: notes follow their rows, stored column settings follow a rename, and
/// the layout, validation and save catch up.
fn settle(
    state: &Entity<TableState<QrateTableDelegate>>,
    changes: &[Change],
    origin: Origin,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        // A row put back by a restore carries no photo, and an undone filename names another.
        let rows = changed_rows(state.delegate(), changes);
        photos::refresh(state, rows.as_deref(), cx);
        // The library caches a `Column` per index, so a changed column set needs this.
        state.refresh(cx);
        cx.emit(delegate::TableChanged);
        cx.notify();
    });
    if history::moves_rows(changes) {
        let row_ids = state.read(cx).delegate().row_ids().to_vec();
        diagnostics::Diagnostics::align_note_rows(diagnostics::DATASET_MAIN, &row_ids, origin, cx);
    }
    for change in changes {
        if let Change::ColumnRenamed { before, after } = change {
            follow_rename(before, &after.clone().into(), cx);
        }
    }
    TablePanel::persist_columns(state, cx);
    sync_project_columns(state, None, cx);
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
    autosave(cx);
}

/// Re-key what is stored under a column's name — its notes, its settings, its `__columns` row —
/// after the grid's column has been renamed.
fn follow_rename(before: &str, after: &SharedString, cx: &mut App) {
    diagnostics::Diagnostics::column_renamed(diagnostics::DATASET_MAIN, before, after, cx);
    settings::columns::rename(before, after, cx);
    if let Some(file) = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|p| p.file.clone())
        && let Err(err) = settings::project::rename_column(&file, before, after)
    {
        log::error!("failed to rename column {before} to {after}: {err}");
    }
}

/// Put the project back the way it was just after entry `to`, by applying the inverse of every
/// change made since, newest first. Nothing is rewound: the restore is itself logged, and undone
/// with one Ctrl+Z.
pub fn restore_to(to: EntryId, cx: &mut App) {
    let Some(file) = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|p| p.file.clone())
    else {
        return;
    };
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let saved = match settings::history::entries_after(&file, to) {
        Ok(saved) => saved,
        Err(err) => {
            log::error!("couldn't read the project history to restore it: {err}");
            return;
        }
    };
    let unsaved = state.read(cx).delegate().unsaved_history().to_vec();
    let (notes, grid): (Vec<Change>, Vec<Change>) = saved
        .iter()
        .chain(&unsaved)
        .rev()
        .flat_map(|entry| entry.changes.iter().rev().map(Change::inverse))
        .partition(|change| matches!(change, Change::Note { .. }));

    let applied = state.update(cx, |state, _| state.delegate_mut().restore(&grid, to));
    settle(&state, &applied, Origin::Restore(to), cx);

    for note in notes {
        let Change::Note {
            row, column, after, ..
        } = note
        else {
            continue;
        };
        let position = row.and_then(|id| {
            let delegate = state.read(cx).delegate();
            delegate.row_ids().iter().position(|r| *r == id)
        });
        if row.is_some() && position.is_none() {
            continue;
        }
        let location = diagnostics::Location {
            dataset: diagnostics::DATASET_MAIN.into(),
            row: position,
            row_id: row,
            column: column.map(Into::into),
        };
        diagnostics::Diagnostics::set_note(
            location,
            after.unwrap_or_default().into(),
            Origin::Restore(to),
            cx,
        );
    }
}

/// Put one cell back to the value an entry recorded, wherever that row and column now sit.
pub fn restore_value(
    row: settings::project::RowId,
    column: &str,
    text: SharedString,
    from: EntryId,
    cx: &mut App,
) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let target = {
        let delegate = state.read(cx).delegate();
        delegate
            .row_ids()
            .iter()
            .position(|id| *id == row)
            .zip(delegate.data_col(column))
    };
    if let Some((row, col)) = target {
        write_cell(row, col, text, Origin::Restore(from), cx);
    }
}

/// A change to the grid's shape rather than its contents. One enum rather than five entry points
/// because every one of them has the same tail: re-index the notes, re-save the column layout,
/// revalidate, mark dirty, save.
pub enum Structural {
    InsertRow {
        at: usize,
    },
    /// Copy a row in below itself — the one insert that starts with content.
    DuplicateRow {
        row: usize,
    },
    /// Source row indices, in any order.
    DeleteRows(Vec<usize>),
    InsertColumn {
        at: usize,
    },
    DeleteColumn {
        col: usize,
    },
    RenameColumn {
        col: usize,
        name: SharedString,
    },
}

pub enum Arrangement {
    Indent(usize),
    /// Wrap these source rows in a new parent.
    Group(Vec<usize>),
    Outdent(usize),
    Reparent {
        row: usize,
        parent: Option<usize>,
    },
    DeleteSubtree(usize),
    Move {
        row: usize,
        target: usize,
        placement: RowPlacement,
    },
}

#[derive(Clone, Copy)]
pub enum RowPlacement {
    Before,
    Child,
    After,
}

pub fn arrange(op: Arrangement, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    if let Arrangement::DeleteSubtree(row) = op {
        let rows = state.read(cx).delegate().subtree_sources(row);
        structural(Structural::DeleteRows(rows), cx);
        return;
    }
    let result = state.update(cx, |state, cx| {
        let result = match op {
            Arrangement::Indent(row) => state.delegate_mut().indent_row(row),
            Arrangement::Group(rows) => state.delegate_mut().group_rows(&rows),
            Arrangement::Outdent(row) => state.delegate_mut().outdent_row(row),
            Arrangement::Reparent { row, parent } => state.delegate_mut().reparent_row(row, parent),
            Arrangement::Move {
                row,
                target,
                placement,
            } => state
                .delegate_mut()
                .move_row_relative(row, target, placement),
            Arrangement::DeleteSubtree(_) => unreachable!(),
        };
        if result.is_ok() {
            state.refresh(cx);
            cx.emit(delegate::TableChanged);
            cx.notify();
        }
        result
    });
    if let Err(error) = result {
        log::warn!("Could not change archival hierarchy: {error:?}");
        return;
    }
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    autosave(cx);
}

pub(crate) fn persist_expanded(rows: &[settings::project::RowId], cx: &mut App) {
    if !cx.has_global::<settings::project::CurrentProject>() {
        return;
    }
    settings::project::CurrentProject::set_text(
        settings::description::HIERARCHY_EXPANDED_KEY,
        serde_json::to_string(rows)
            .unwrap_or_else(|_| "[]".into())
            .into(),
        cx,
    );
}

/// Hand the grid's column names to the open project, which is what every other reader of the
/// column list looks at.
fn sync_project_columns(
    state: &Entity<TableState<QrateTableDelegate>>,
    renamed: Option<&(SharedString, SharedString)>,
    cx: &mut App,
) {
    let delegate = state.read(cx).delegate();
    let headers = (0..delegate.column_count())
        .map(|col| delegate.column_name(col).to_string())
        .collect();
    let renamed = renamed.map(|(before, after)| (before.as_ref(), after.as_ref()));
    settings::project::CurrentProject::set_columns(headers, renamed, cx);
}

/// Apply a shape change to the table and everything keyed off the shape.
///
/// Validation runs *now*, not on `TablePanel`'s debounce: after a row shift the open diagnostics
/// don't merely lag, they point at the wrong rows.
pub fn structural(op: Structural, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };

    // What the notes have to follow, decided before the delegate's indices move under us.
    let renamed = match &op {
        Structural::RenameColumn { col, name } => {
            let before = state.read(cx).delegate().column_name(*col);
            (before != *name).then(|| (before, name.clone()))
        }
        _ => None,
    };

    let applied = state.update(cx, |state, cx| {
        let delegate = state.delegate_mut();
        let applied = match &op {
            Structural::InsertRow { at } => {
                delegate.insert_rows(*at, None);
                true
            }
            Structural::DuplicateRow { row } => {
                delegate.insert_rows(row + 1, Some(*row));
                true
            }
            Structural::DeleteRows(rows) => {
                delegate.remove_rows(rows);
                true
            }
            Structural::InsertColumn { at } => {
                delegate.insert_column(*at);
                true
            }
            Structural::DeleteColumn { col } => {
                delegate.remove_column(*col);
                true
            }
            Structural::RenameColumn { col, name } => delegate.rename_column(*col, name.clone()),
        };
        if applied {
            // The library caches a `Column` per index, so a changed column set needs this.
            state.refresh(cx);
            cx.emit(delegate::TableChanged);
            cx.notify();
        }
        applied
    });
    if !applied {
        log::warn!("a column already goes by that name — the rename was left alone");
        return;
    }

    if matches!(
        op,
        Structural::InsertRow { .. } | Structural::DuplicateRow { .. } | Structural::DeleteRows(_)
    ) {
        let row_ids = state.read(cx).delegate().row_ids().to_vec();
        diagnostics::Diagnostics::align_note_rows(
            diagnostics::DATASET_MAIN,
            &row_ids,
            Origin::Structure,
            cx,
        );
    }
    if matches!(
        op,
        Structural::InsertColumn { .. } | Structural::DeleteColumn { .. }
    ) || renamed.is_some()
    {
        sync_project_columns(&state, renamed.as_ref(), cx);
    }
    if let Some((before, after)) = renamed {
        follow_rename(&before, &after, cx);
    }

    // The saved layout is only applied when its keys are a permutation of the live ones, so a
    // stale one would be thrown away wholesale on the next open, taking the widths with it.
    TablePanel::persist_columns(&state, cx);
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
    autosave(cx);
}

/// Every cell of one row, blanked — what the row header's "Clear row" writes.
pub(crate) fn blank_row(row: usize, cx: &App) -> Vec<(usize, usize, SharedString)> {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return Vec::new();
    };
    let columns = state.read(cx).delegate().column_count();
    (0..columns)
        .map(|col| (row, col, SharedString::default()))
        .collect()
}

/// Freeze the leading `count` data columns and remember it in the open project. `count` is a
/// display-order count, so unfreezing is just this with zero. Without a project there's nowhere to
/// persist it, and the freeze lasts until the window closes.
pub(crate) fn set_frozen_columns(
    table: &Entity<TableState<QrateTableDelegate>>,
    count: usize,
    cx: &mut App,
) {
    let clamped = table.update(cx, |state, cx| {
        state.delegate_mut().set_frozen(count);
        // The library caches a `Column` per index; without this the fixed region doesn't move.
        state.refresh(cx);
        cx.notify();
        state.delegate().frozen()
    });
    if cx.has_global::<settings::project::CurrentProject>() {
        settings::project::CurrentProject::set_text(
            panel::FROZEN_COLUMNS_KEY,
            clamped.to_string().into(),
            cx,
        );
    }
}

/// The text a diagnostic points at, addressed the way a diagnostic is — by column *name*, since
/// that is what survives a column move. `None` for anything but a cell.
pub fn cell_text(location: &diagnostics::Location, cx: &App) -> Option<SharedString> {
    let state = cx.try_global::<TableStateHandle>()?.0.upgrade()?;
    let delegate = state.read(cx).delegate();
    let col = delegate.data_col(location.column.as_ref()?)?;
    delegate.cell(location.row?, col).cloned()
}

/// [`cell_text`]'s other half: write text back to whatever a diagnostic points at.
pub fn set_cell_text(
    location: &diagnostics::Location,
    text: SharedString,
    origin: Origin,
    cx: &mut App,
) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let target = {
        let delegate = state.read(cx).delegate();
        location
            .column
            .as_ref()
            .and_then(|name| delegate.data_col(name))
            .zip(location.row)
    };
    if let Some((col, row)) = target {
        write_cell(row, col, text, origin, cx);
    }
}

/// Resolve several diagnostic locations through one table edit.
pub fn set_cell_texts(
    replacements: Vec<(diagnostics::Location, SharedString)>,
    origin: Origin,
    cx: &mut App,
) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let cells = {
        let state = state.read(cx);
        let delegate = state.delegate();
        replacements
            .into_iter()
            .filter_map(|(location, text)| {
                delegate
                    .data_col(location.column.as_ref()?)
                    .zip(location.row)
                    .map(|(col, row)| ((row, col), text))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_iter()
            .map(|((row, col), text)| (row, col, text))
            .collect()
    };
    write_cells(cells, origin, cx);
}

/// What a command invoked from outside the table acts on: the selected column, or nothing at all.
///
/// A bar item has no column under it the way a right-click menu does, so the selection is the only
/// answer available — and "no selection" has to stay tellable from "an empty column", which is why
/// every column field here is optional.
pub fn selected_context(plugin: &SharedString, cx: &App) -> CommandContext {
    let selected = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
        .and_then(|state| {
            let state = state.read(cx);
            let col = match state.delegate().selection()? {
                Selection::Cell { col, .. } | Selection::Column(col) => col,
                Selection::Row(_) => return None,
            };
            let delegate = state.delegate();
            Some((
                delegate.column_key(col),
                delegate.column_name(col),
                delegate.column_cells(col),
            ))
        });

    let Some((key, name, values)) = selected else {
        return CommandContext::default();
    };
    CommandContext {
        column_settings: settings::columns::get(&key, cx)
            .plugins
            .get(plugin.as_ref())
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        column: Some(name),
        column_key: Some(key),
        row: None,
        values,
        argument: None,
    }
}

/// Persist the open project's table data to its `.qrate` file, synchronously — Ctrl+S and quit,
/// which must not return before the data is on disk. Waits out a background autosave still
/// writing. Writes only the rows the unsaved log names unless the columns changed.
///
/// `Ok` clears the `PROJECT_DATA` dirty mark, including when no project is open. A project whose
/// table is unavailable cannot be saved; `Err` leaves the mark and says why, for the archivist.
pub fn save_now(cx: &mut App) -> Result<(), String> {
    stamp_pending_author(cx);
    if !cx.has_global::<settings::project::CurrentProject>() {
        settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
        return Ok(());
    }
    let Some((file, state)) = save_target(cx) else {
        return Err("Couldn't save your changes: the project's table is unavailable.".into());
    };
    let started = std::time::Instant::now();
    let snapshot = state.read(cx).delegate().save_snapshot();
    let (rows, edits) = (snapshot.rows.len(), snapshot.edits);
    let written = write_snapshot(&file, &snapshot);
    saved(&state, &file, edits, written, cx)?;
    log::debug!("saved {rows} rows in {:?}", started.elapsed());
    Ok(())
}

/// What the workspace's Getting started guide reads off the grid to tell which of its tasks are
/// already done. Read rather than reported: the grid has no idea a guide exists, and a task an
/// archivist finished on their own (typing a title before being asked) should still tick.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct GuideFacts {
    /// Source rows, including filtered-away ones.
    pub rows: usize,
    /// Data columns.
    pub columns: usize,
    /// Whether any row has text in the Title column (the first column, when none is declared).
    pub any_title: bool,
    /// Whether a row or cell is selected — what opens a row in Details.
    pub selected: bool,
    /// Rows whose file resolved to something on disk.
    pub linked_files: usize,
}

/// The data column holding each row's title: the one declared Title, else the first.
fn title_col(delegate: &QrateTableDelegate) -> usize {
    (0..delegate.column_count())
        .find(|&col| delegate.column_type(col) == ColumnType::Title)
        .unwrap_or(0)
}

/// See [`GuideFacts`]. `None` before the grid exists.
pub fn guide_facts(cx: &App) -> Option<GuideFacts> {
    let state = cx.try_global::<TableStateHandle>()?.0.upgrade()?;
    let delegate = state.read(cx).delegate();
    let rows = delegate.row_count();
    let title = title_col(delegate);
    Some(GuideFacts {
        rows,
        columns: delegate.column_count(),
        any_title: (0..rows).any(|row| {
            delegate
                .cell(row, title)
                .is_some_and(|text| !text.trim().is_empty())
        }),
        selected: matches!(
            delegate.selection(),
            Some(Selection::Cell { .. } | Selection::Row(_))
        ),
        linked_files: (0..rows)
            .filter(|&row| delegate.row_image(row).is_some())
            .count(),
    })
}

/// Select the first visible row — its Title cell when `title_cell`, else the whole row — the way
/// a click would, so Details follows. The guide's "Show me" for the tasks that start in the grid.
pub fn select_first_row(title_cell: bool, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    let target = {
        let delegate = state.read(cx).delegate();
        delegate.visible().first().map(|_| (0, title_col(delegate)))
    };
    let Some((view_row, col)) = target else {
        return;
    };
    state.update(cx, |state, cx| {
        if title_cell {
            // `+ 1`: the library counts the pinned row-number column, the data does not.
            state.set_selected_cell(view_row, col + 1, cx)
        } else {
            state.set_selected_row(view_row, cx)
        }
    });
}
