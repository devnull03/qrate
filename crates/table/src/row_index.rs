//! The pinned row-number column at table column 0. Display-only — never routes through
//! `selection.rs`/`editing.rs`, so it can't be selected or edited.

use std::sync::OnceLock;

use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{
    ActiveTheme,
    table::{Column, TableState},
};

use crate::delegate::QrateTableDelegate;

/// Table-column index of the pinned row-number column. Data columns start at 1.
pub(crate) const COL_IX: usize = 0;

const WIDTH: f32 = 48.;

pub(crate) fn column() -> &'static Column {
    static COLUMN: OnceLock<Column> = OnceLock::new();
    COLUMN.get_or_init(|| {
        Column::new("__row_ix", "")
            .fixed_left()
            .resizable(false)
            .movable(false)
            .selectable(false)
            .width(px(WIDTH))
    })
}

pub(crate) fn render_td(
    row_ix: usize,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> gpui::AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from((row_ix + 1).to_string()))
        .into_any_element()
}
