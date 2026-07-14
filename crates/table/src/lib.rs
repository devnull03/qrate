//! The center table panel: a virtualized data grid with a pinned row-number column, native
//! cell/row/column selection, movable + resizable columns (layout persisted per project), and
//! double-click-to-edit cells. This crate owns all table state; `workspace` only needs to know
//! `TablePanel` is a `gpui_component::dock::Panel` it can register and place, and `app`'s
//! status-bar items only need `TableStateHandle`/`QrateTableDelegate` to read the selection.

mod cell;
mod delegate;
mod editing;
mod panel;
mod row_index;

pub use delegate::{QrateTableDelegate, Selection, TableChanged};
pub use panel::TablePanel;

/// Settings key (in either scope) for the alternating-row-stripe toggle.
pub const TABLE_STRIPES_KEY: &str = "table_stripes";

use gpui::{Global, WeakEntity};
use gpui_component::table::TableState;

/// Global handle to the live table state, so cross-crate status-bar items (the fake-data button
/// and the selected-cell widget in the `app` crate) can reach the table.
pub struct TableStateHandle(pub WeakEntity<TableState<QrateTableDelegate>>);
impl Global for TableStateHandle {}
