use std::sync::OnceLock;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, FontWeight, InteractiveElement as _, IntoElement, SharedString, div, prelude::*, px,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{
    ActiveTheme,
    table::{Column, TableState},
};

use diagnostics::Diagnostics;

use crate::delegate::QrateTableDelegate;
use crate::note::{self, Target};

pub(crate) const COL_IX: usize = 0;

const WIDTH: f32 = 88.;

#[derive(Clone)]
struct RowDrag(usize);

struct RowDragPreview(usize);

impl gpui::Render for RowDragPreview {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().primary)
            .child(format!("Move component {}", self.0 + 1))
    }
}

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
    delegate: &QrateTableDelegate,
    view_ix: usize,
    row_ix: usize,
    highlighted: bool,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> gpui::AnyElement {
    let (fg, bg) = if highlighted {
        (cx.theme().foreground, cx.theme().secondary_hover)
    } else {
        (cx.theme().muted_foreground, cx.theme().transparent)
    };
    let location = delegate.location(Some(row_ix), None);
    let worst = Diagnostics::worst_at(&location.dataset, Some(row_ix), None, cx);
    let tip = note::tooltip_text(&location, cx);
    let depth = delegate.row_depth(view_ix);
    let has_children = delegate.row_has_children(row_ix);
    let expanded = delegate.row_expanded(row_ix);

    div()
        .id(("row-note", row_ix))
        .size_full()
        .relative()
        .flex()
        .items_center()
        .pl(px(6. + depth.min(4) as f32 * 12.))
        .gap_1()
        .on_drag(RowDrag(row_ix), move |drag, _, _, cx| {
            cx.new(|_| RowDragPreview(drag.0))
        })
        .child(
            div().absolute().inset_0().flex().flex_col().children(
                [
                    crate::RowPlacement::Before,
                    crate::RowPlacement::Child,
                    crate::RowPlacement::After,
                ]
                .map(|placement| {
                    div()
                        .flex_1()
                        .drag_over::<RowDrag>(|style, _, _, cx| style.bg(cx.theme().table_active))
                        .on_drop(move |drag: &RowDrag, window, cx| {
                            let source = drag.0;
                            window.defer(cx, move |_, cx| {
                                crate::arrange(
                                    crate::Arrangement::Move {
                                        row: source,
                                        target: row_ix,
                                        placement,
                                    },
                                    cx,
                                );
                            });
                        })
                }),
            ),
        )
        .bg(bg)
        .text_color(fg)
        .when(highlighted, |d| d.font_weight(FontWeight::SEMIBOLD))
        .child(
            div()
                .id(("row-chevron", row_ix))
                .w(px(14.))
                .cursor_pointer()
                .when(has_children, |chevron| {
                    chevron
                        .child(if expanded { "▾" } else { "▸" })
                        .on_click(cx.listener(move |state, _, _, cx| {
                            state.delegate_mut().toggle_expanded(row_ix);
                            let expanded = state.delegate().expanded_rows();
                            state.refresh(cx);
                            crate::persist_expanded(&expanded, cx);
                            cx.notify();
                        }))
                }),
        )
        .child(SharedString::from((row_ix + 1).to_string()))
        .when_some(worst, |d, severity| d.child(note::marker(severity, cx)))
        .when_some(tip, |d, text| {
            d.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .tooltip_show_delay(note::HOVER_DELAY)
        })
        .context_menu(move |menu, window, cx| note::menu(Target::Row(row_ix), menu, window, cx))
        .when_some(
            note::editor(delegate, Some(row_ix), None, cx),
            |d, editor| d.child(editor),
        )
        .into_any_element()
}
