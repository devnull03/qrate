//! Data-cell rendering: plain text, swapped for the shared inline editor while that cell is
//! being edited (see `editing.rs`). Selection is the table's own native cell selection
//! (`cell_selectable`) — the library draws the active-cell highlight and emits the
//! `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, deferred, div, px,
};
use gpui_component::{ActiveTheme as _, input::Input, table::TableState};

use crate::{delegate::QrateTableDelegate, editing::EditState, floating::clamped_float};

/// `row_ix` is a source (not view) row index — `render_td` maps through `visible_rows` first.
/// `col_ix` is a data-column index, not shifted for the pinned row-index column.
pub(crate) fn render_cell(
    delegate: &mut QrateTableDelegate,
    row_ix: usize,
    col_ix: usize,
    _window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let editing = delegate.editing
        == (EditState::Editing {
            row: row_ix,
            col: col_ix,
        });

    // Keep the plain text underneath so the row height and neighbouring cells are unaffected; the
    // editor floats over it.
    let text = delegate.cell(row_ix, col_ix).cloned().unwrap_or_default();
    // The table's rect (measured in `panel.rs`) both caps the editor's wrap width — so long text
    // wraps multi-line instead of scrolling sideways (height is `auto_grow`'s job) — and is the
    // rect `clamped_float` keeps the box inside.
    let table = cx
        .try_global::<crate::TableViewportBounds>()
        .map(|b| b.0)
        .unwrap_or_default();
    let max_w = table.size.width;
    div()
        .size_full()
        .child(text)
        .when(editing, |cell| {
            // `deferred` paints the box above the grid and lets it escape the cell's clip;
            // `clamped_float` confines it to the *table* rect (not the window), so it can't spill
            // over the details or any other side panel. Dismissed only by the user (Enter/blur
            // commit, Escape, or a filter dropping the row) — never by scrolling.
            cell.child(deferred(clamped_float(
                table,
                div()
                    // Swallow mouse events so clicking/selecting inside the editor doesn't fall
                    // through to the cells painted behind it (moving the table's selection).
                    .occlude()
                    // Triple the old 240px minimum so the box opens comfortably wide, but never
                    // past the panel width (`clamped_float` then shifts it left to fit at the edge).
                    .min_w(max_w.min(px(720.)))
                    .max_w(max_w)
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .shadow_lg()
                    .child(Input::new(&delegate.editor)),
            )))
        })
        .into_any_element()
}
