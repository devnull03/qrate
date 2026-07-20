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
mod panel;
pub mod photos;
mod row_index;

pub use delegate::{QrateTableDelegate, Selection, TableChanged};
pub use panel::{Search, TablePanel};

/// Settings key (in either scope) for the alternating-row-stripe toggle.
pub const TABLE_STRIPES_KEY: &str = "table_stripes";

use gpui::{App, Bounds, Global, Pixels, WeakEntity, px, size};
use gpui_component::table::TableState;

/// Global handle to the live table state, so cross-crate status-bar items (the fake-data button
/// and the selected-cell widget in the `app` crate) can reach the table.
pub struct TableStateHandle(pub WeakEntity<TableState<QrateTableDelegate>>);
impl Global for TableStateHandle {}

/// The table area's window-space rectangle, measured each frame (see `panel.rs`). The floating
/// cell editor caps its wrap width to this and clamps its position to it, so a long value wraps
/// within the panel instead of scrolling sideways and the box never spills over a side panel.
pub(crate) struct TableViewportBounds(pub Bounds<Pixels>);
impl Global for TableViewportBounds {}

impl Default for TableViewportBounds {
    /// A rect large enough to be a no-op clamp until the first real measurement lands (editing
    /// can't start before the panel has rendered once anyway).
    fn default() -> Self {
        Self(Bounds::new(Default::default(), size(px(4000.), px(4000.))))
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
        Err(err) => eprintln!("failed to save project data: {err}"),
    }
}
