//! The center table panel: a virtualized data grid with a pinned row-number column, single-cell
//! selection, and double-click-to-edit cells. This crate owns all table state; `workspace` only
//! needs to know `TablePanel` is a `gpui_component::dock::Panel` it can register and place, and
//! `app`'s status-bar items only need `TableStateHandle`/`QrateTableDelegate` to read selection.

mod delegate;
mod editing;
mod panel;
mod row_index;
mod selection;

pub use delegate::{QrateTableDelegate, TableChanged};
pub use panel::TablePanel;

use gpui::{Global, WeakEntity};
use gpui_component::table::TableState;

/// Global handle to the live table state, so cross-crate status-bar items (the fake-data button
/// and the selected-cell widget in the `app` crate) can reach the table.
pub struct TableStateHandle(pub WeakEntity<TableState<QrateTableDelegate>>);
impl Global for TableStateHandle {}
