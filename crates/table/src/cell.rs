//! Data-cell rendering: plain text, swapped for the shared inline editor while that cell is
//! being edited (see `editing.rs`). Selection is the table's own native cell selection
//! (`cell_selectable`) — the library draws the active-cell highlight and emits the
//! `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnchoredPositionMode, AnyElement, Context, IntoElement, ParentElement as _, Styled as _,
    Window, anchored, deferred, div, px,
};
use gpui_component::{ActiveTheme as _, input::Input, table::TableState};

use crate::{delegate::QrateTableDelegate, editing::EditState};

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
    div()
        .size_full()
        .child(text)
        .when(editing, |cell| {
            // A floating editor over the cell: `deferred` + `anchored` (Local) paint it above the
            // grid and let it overflow the cramped cell, while `snap_to_window` keeps it fully
            // on-screen whichever edge the cell sits near. Dismissed only by the user (Enter/blur
            // commit, Escape, or a filter dropping the row) — never by scrolling.
            cell.child(deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Local)
                    .snap_to_window()
                    .child(
                        div()
                            .min_w(px(240.))
                            .max_w(px(520.))
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .shadow_lg()
                            .child(Input::new(&delegate.editor)),
                    ),
            ))
        })
        .into_any_element()
}
