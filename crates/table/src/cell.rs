//! Data-cell rendering: plain text, swapped for the shared inline editor while that cell is
//! being edited (see `editing.rs`). Selection is the table's own native cell selection
//! (`cell_selectable`) — the library draws the active-cell highlight and emits the
//! `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window, div};
use gpui_component::{input::Input, table::TableState};

use crate::{delegate::QrateTableDelegate, editing::EditState};

/// `row_ix` is a source (not view) row index — `render_td` maps the library's view index
/// through `visible_rows` before calling here, so the edit-in-progress check and the text
/// lookup both key off source data. `col_ix` is a data-column index, not shifted for the pinned
/// row-index column.
pub(crate) fn render_cell(
    delegate: &mut QrateTableDelegate,
    row_ix: usize,
    col_ix: usize,
    _window: &mut Window,
    _cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    if delegate.editing
        == (EditState::Editing {
            row: row_ix,
            col: col_ix,
        })
    {
        return Input::new(&delegate.editor).into_any_element();
    }

    let text = delegate.cell(row_ix, col_ix).cloned().unwrap_or_default();
    div().size_full().child(text).into_any_element()
}
