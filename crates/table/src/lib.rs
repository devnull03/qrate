//! The center table panel: a virtualized data grid with a pinned row-number column, native
//! cell/row/column selection, movable + resizable columns (layout persisted per project), and
//! double-click-to-edit cells. This crate owns all table state; `workspace` only needs to know
//! `TablePanel` is a `gpui_component::dock::Panel` it can register and place, and `app`'s
//! status-bar items only need `TableStateHandle`/`QrateTableDelegate` to read the selection.

mod cell;
mod delegate;
mod editing;
mod filter;
mod floating;
mod note;
mod panel;
pub mod photos;
mod row_index;

pub use delegate::{QrateTableDelegate, Selection, TableChanged};
pub use panel::{Search, TablePanel};

/// Settings key (in either scope) for the alternating-row-stripe toggle.
pub const TABLE_STRIPES_KEY: &str = "table_stripes";

use gpui::{App, Bounds, Global, Pixels, Point, SharedString, WeakEntity, px, size};
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

/// Where the in-progress cell edit opened: which cell, its full window-space rect, and the table's
/// scroll offset at that instant. Written once per edit by `cell.rs` (the cell is on screen when an
/// edit starts), read by `panel.rs` to place and size the floating editor — which is why the box
/// stays put when the grid scrolls out from under it, Sheets-style. `scroll` is filled in on the
/// next panel render, since the scroll handles live on `TableState` and aren't reachable from a
/// cell. Cleared by `editing::start` so re-editing the same cell re-measures.
pub(crate) struct EditSpawn {
    pub cell: (usize, usize),
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

/// Commit `text` into a cell, mark the project dirty, and re-run validation — everything a
/// committed edit does except going through the inline editor. What a fix menu applies through.
pub fn write_cell(row: usize, col: usize, text: SharedString, cx: &mut App) {
    let Some(state) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };
    state.update(cx, |state, cx| {
        state.delegate_mut().set_cell(row, col, text);
        cx.emit(delegate::TableChanged);
        cx.notify();
    });
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    revalidate_now(cx);
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
    let (headers, rows) = state.read(cx).delegate().dataset_snapshot();
    match settings::project::save_dataset(&file, &headers, &rows) {
        Ok(()) => settings::dirty::clear(settings::dirty::PROJECT_DATA, cx),
        Err(err) => log::error!("failed to save project data: {err}"),
    }
}
