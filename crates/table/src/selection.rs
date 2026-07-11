//! Single-cell selection: `gpui_component`'s table only supports independent row/column
//! selection axes (row body click vs. column header click), with no combined "select this
//! exact cell" click. This module owns the click handler that sets both at once (into our own
//! `QrateTableDelegate::selected`, not the library's `selected_row`/`selected_col`, since
//! calling those together would also draw the library's whole-row highlight box, which fights
//! the single-cell highlight below). A double-click starts editing (`editing::start`).

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme, input::Input, table::TableState};

use crate::{
    delegate::{QrateTableDelegate, TableChanged},
    editing,
    editing::EditState,
};

pub(crate) fn render_cell(
    delegate: &mut QrateTableDelegate,
    row_ix: usize,
    col_ix: usize,
    _window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    if delegate.editing
        == (EditState::Editing {
            row: row_ix,
            col: col_ix,
        })
    {
        return Input::new(&delegate.editor).into_any_element();
    }

    let selected = delegate.selected == Some((row_ix, col_ix));
    let text = delegate.cell(row_ix, col_ix).cloned().unwrap_or_default();

    div()
        .id(("table-cell", row_ix * 1_000_000 + col_ix))
        .size_full()
        .when(selected, |this| this.bg(cx.theme().table_active))
        .child(text)
        .on_click(cx.listener(move |state, e: &ClickEvent, window, cx| {
            state.delegate_mut().selected = Some((row_ix, col_ix));
            if e.click_count() == 2 {
                editing::start(state.delegate_mut(), row_ix, col_ix, window, cx);
            }
            cx.emit(TableChanged);
            cx.notify();
        }))
        .into_any_element()
}
