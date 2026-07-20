//! The pinned row-number column at table column 0. Display-only — never routes through
//! `cell.rs`/`editing.rs`, so it can't be edited; `TablePanel`'s event bridge bounces any
//! native cell selection landing here onto the first data column.

use std::sync::OnceLock;

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{
    ActiveTheme,
    table::{Column, TableState},
};

use crate::delegate::QrateTableDelegate;

/// Table-column index of the pinned row-number column. Data columns start at 1.
pub(crate) const COL_IX: usize = 0;

const WIDTH: f32 = 48.;

pub(crate) fn column() -> Column {
    static COLUMN: OnceLock<Column> = OnceLock::new();
    COLUMN
        .get_or_init(|| {
            Column::new("__row_ix", "")
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false)
                .width(px(WIDTH))
        })
        .clone()
}

pub(crate) fn render_td(
    row_ix: usize,
    highlighted: bool,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> gpui::AnyElement {
    // Active row's number reads in full-strength text on a filled cell (like a sheet's row
    // header); other rows stay muted. The `#` column is `fixed_left`, so this stays visible and
    // highlighted while scrolling horizontally.
    let (fg, bg) = if highlighted {
        (cx.theme().foreground, cx.theme().secondary_hover)
    } else {
        (cx.theme().muted_foreground, cx.theme().transparent)
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .text_color(fg)
        .when(highlighted, |d| d.font_weight(FontWeight::SEMIBOLD))
        .child(SharedString::from((row_ix + 1).to_string()))
        .into_any_element()
}
