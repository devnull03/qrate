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
pub mod file_links;
mod filter;
pub mod floating;
mod history;
mod note;
mod panel;
pub mod photos;
mod row_index;

pub use agent::{AGENT_SOURCE, respond_to_agent, respond_to_agent_async};
pub use delegate::{QrateTableDelegate, Selection, TableChanged};
/// The grid's right-click menu, so a gallery card can raise the same one a row does rather than
/// growing a second, quietly diverging copy.
pub use note::{Target as MenuTarget, menu as context_menu};
pub use panel::{
    Clear, Copy, Cut, DeleteColumn, DeleteRow, Deselect, DuplicateRow, EditCell, GRID_CONTEXT,
    InsertColumnLeft, InsertColumnRight, InsertNote, InsertRowAbove, InsertRowBelow, Paste, Redo,
    RenameColumn, Replace, Search, TablePanel, Undo, UnfreezeColumns, editor_box,
};

/// Settings key (in either scope) for the alternating-row-stripe toggle.
pub const TABLE_STRIPES_KEY: &str = "table_stripes";

use gpui::{
    App, Bounds, ClipboardItem, Entity, Global, Pixels, Point, SharedString, WeakEntity, px, size,
};
use gpui_component::table::TableState;
use plugin_api::CommandContext;

/// Global handle to the live table state, so cross-crate status-bar items (the fake-data button
/// and the selected-cell widget in the `app` crate) can reach the table.
pub struct TableStateHandle(pub WeakEntity<TableState<QrateTableDelegate>>);
impl Global for TableStateHandle {}

/// The table area's window-space rectangle, measured each frame (see `panel.rs`). It's both the
/// origin the floating cell editor is positioned against and the limit it grows to, so a long
/// value wraps within the panel instead of spilling over a side panel. The note editor clamps to
/// it too, via `clamped_float`.
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
pub fn revalidate_now(cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    state.update(cx, |state, cx| state.delegate().revalidate(cx));
}

/// Save now unless autosave is switched off, for the paths that act on an explicitly clicked
/// target rather than on typing.
///
/// No debounce, unlike `TablePanel`'s: the timer exists to coalesce a burst of keystrokes, and a
/// menu click is one deliberate change. That also keeps the pending write off `TablePanel`, which
/// none of these free functions can reach.
fn autosave(cx: &mut App) {
    if settings::effective_text(settings::AUTOSAVE_KEY, cx).as_ref() != "off" {
        save_now(cx);
    }
}

/// Commit `text` into a cell, mark the project dirty, and re-run validation — everything a
/// committed edit does except going through the inline editor. What a fix menu applies through.
pub fn write_cell(row: usize, col: usize, text: SharedString, cx: &mut App) {
    write_cells(vec![(row, col, text)], cx);
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
pub fn write_cells(cells: Vec<(usize, usize, SharedString)>, cx: &mut App) {
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
        state.delegate_mut().apply_edit(cells);
        cx.emit(delegate::TableChanged);
        cx.notify();
    });
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
    autosave(cx);
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
    let row_ids = state.update(cx, |state, cx| {
        let rows_changed = match redo {
            true => state.delegate_mut().redo(),
            false => state.delegate_mut().undo(),
        };
        if rows_changed.is_some() {
            cx.emit(delegate::TableChanged);
            cx.notify();
        }
        rows_changed.map(|changed| changed.then(|| state.delegate().row_ids().to_vec()))
    });
    let Some(row_ids) = row_ids else {
        return;
    };
    if let Some(row_ids) = row_ids {
        diagnostics::Diagnostics::align_note_rows(diagnostics::DATASET_MAIN, &row_ids, cx);
    }
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
    autosave(cx);
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
        diagnostics::Diagnostics::align_note_rows(diagnostics::DATASET_MAIN, &row_ids, cx);
    }
    if let Some((before, after)) = renamed {
        diagnostics::Diagnostics::column_renamed(diagnostics::DATASET_MAIN, &before, &after, cx);
        settings::columns::rename(&before, &after, cx);
        if let Some(file) = cx
            .try_global::<settings::project::CurrentProject>()
            .map(|p| p.file.clone())
            && let Err(err) = settings::project::rename_column(&file, &before, &after)
        {
            log::error!("failed to rename column {before} to {after}: {err}");
        }
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
pub fn set_cell_text(location: &diagnostics::Location, text: SharedString, cx: &mut App) {
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
        write_cell(row, col, text, cx);
    }
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

/// Persist the open project's table data to its `.qrate` file, synchronously, and clear the
/// `PROJECT_DATA` dirty mark. No-op with no project open, no live table, or a blank project. Runs
/// on the calling thread — qrate's grids are small enough that a full rewrite stays well under a
/// frame; move it onto the background executor if large projects ever stutter here.
pub fn save_now(cx: &mut App) {
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
    let (headers, row_ids, rows) = state.read(cx).delegate().dataset_snapshot();
    match settings::project::save_dataset(&file, &headers, &row_ids, &rows) {
        Ok(()) => settings::dirty::clear(settings::dirty::PROJECT_DATA, cx),
        Err(err) => log::error!("failed to save project data: {err}"),
    }
}
