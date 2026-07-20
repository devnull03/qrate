//! Google-Sheets-style per-column filter, rendered in the column header via `render_th`: a
//! dropdown with a value-search box and a checklist of the column's distinct values, where
//! unchecking a value hides its rows. The delegate owns the *excluded* set per column
//! (`QrateTableDelegate::filters`); this module is only the UI over it.
//!
//! Hosted in a `Popover`, not a `PopupMenu` — the latter closes on every item click, so a live
//! checklist can't live in one.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px, size,
};
use gpui_component::{
    ActiveTheme, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    popover::Popover,
    table::TableState,
    v_flex, v_virtual_list,
};

use crate::{TableStateHandle, delegate::QrateTableDelegate};

/// Fixed row height for the value checklist. The virtual list needs every item's size up front,
/// so the row below pins itself to exactly this — a mismatch desyncs scrolling from the content.
const ROW_H: f32 = 28.;
/// Tallest the checklist grows before it scrolls instead.
const LIST_MAX_H: f32 = 240.;
const LIST_W: f32 = 224.;

/// The header cell for a data column: its name, plus the filter dropdown trigger on columns where
/// filtering is switched on. Filtering is opt-in per column (see
/// [`QrateTableDelegate::set_column_filter_enabled`]), so most headers render just the name.
pub(crate) fn render_th(
    delegate: &QrateTableDelegate,
    data_col: usize,
    _window: &mut Window,
    _cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let name = delegate.column_name(data_col);
    let active = delegate.column_has_filter(data_col);
    // Highlight this header while its column holds the active (edited/selected) cell. The header
    // row is sticky, so the highlight stays put as the grid scrolls under it.
    let editing_col = delegate.active_cell().is_some_and(|(_, c)| c == data_col);

    h_flex()
        .size_full()
        .justify_between()
        .items_center()
        .gap_1()
        .when(editing_col, |th| th.bg(_cx.theme().secondary_hover))
        .child(div().flex_1().truncate().child(name))
        .when(delegate.column_filter_enabled(data_col), |th| {
            th.child(
                Popover::new(("col-filter", data_col))
                    .trigger(
                        Button::new(("col-filter-btn", data_col))
                            // The bundled icon set has no funnel; `settings-2` is its slider
                            // glyph, the usual stand-in for "filter/adjust".
                            .icon(IconName::Settings2)
                            .ghost()
                            .xsmall()
                            // Highlight the affordance while the column is narrowing the view.
                            .selected(active),
                    )
                    .content(move |_state, window, cx| filter_dropdown(data_col, window, cx)),
            )
        })
        .into_any_element()
}

/// The dropdown body: value search, all/none, and the checklist. Rebuilt on every render (the
/// `Popover` re-invokes this), so it reads live state off the table global rather than capturing it.
fn filter_dropdown(
    data_col: usize,
    _window: &mut Window,
    cx: &mut Context<gpui_component::popover::PopoverState>,
) -> AnyElement {
    let Some(table) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return div().into_any_element();
    };

    // Pull everything the list needs as owned data first, so no borrow of `cx`/the table is held
    // while the child elements are built.
    let (values, filter_search, filter_scroll) = {
        let state = table.read(cx);
        let delegate = state.delegate();
        (
            delegate.column_values(data_col),
            delegate.filter_search.clone(),
            delegate.filter_scroll.clone(),
        )
    };
    let needle = filter_search.read(cx).value().to_lowercase();
    let shown: Vec<SharedString> = values
        .into_iter()
        .filter(|v| needle.is_empty() || v.to_lowercase().contains(&needle))
        .collect();
    let excluded: Vec<bool> = {
        let state = table.read(cx);
        shown
            .iter()
            .map(|v| state.delegate().is_filter_excluded(data_col, v))
            .collect()
    };

    let bulk = |label: &'static str, id: &'static str, exclude_all: bool| {
        let table = table.clone();
        Button::new((id, data_col))
            .outline()
            .xsmall()
            .label(label)
            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                table.update(cx, |s, cx| {
                    if exclude_all {
                        s.delegate_mut().exclude_all_in_column(data_col);
                    } else {
                        s.delegate_mut().clear_column_filter(data_col);
                    }
                    cx.emit(crate::TableChanged);
                    cx.notify();
                });
            })
    };

    let (hover, muted) = (cx.theme().secondary_hover, cx.theme().muted_foreground);
    let (border, radius) = (cx.theme().border, cx.theme().radius);
    let count = shown.len();
    let item_sizes = Rc::new(vec![size(px(LIST_W), px(ROW_H)); count]);
    let view = cx.entity();

    v_flex()
        .w(px(240.))
        .gap_2()
        .child(Input::new(&filter_search).small())
        .child(
            h_flex()
                .gap_1()
                .child(bulk("All", "filter-all", false))
                .child(bulk("None", "filter-none", true)),
        )
        .child(
            div()
                .h(px((count as f32 * ROW_H).clamp(ROW_H, LIST_MAX_H)))
                .rounded(radius)
                .border_1()
                .border_color(border)
                .overflow_x_hidden()
                // Rows truncate, so there is nothing to pan to sideways — swallow horizontal
                // wheel instead of letting it bubble out and scroll the table underneath.
                .on_scroll_wheel(|ev, window, cx| {
                    let d = ev.delta.pixel_delta(window.line_height());
                    if d.x.abs() > d.y.abs() {
                        cx.stop_propagation();
                    }
                })
                .map(|list| {
                    if count == 0 {
                        return list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_color(muted)
                                .child("No matching values"),
                        );
                    }
                    list.child(
                        v_virtual_list(
                            view,
                            ("filter-values", data_col),
                            item_sizes,
                            move |_, range, _, _| {
                                range
                                    .map(|ix| {
                                        row(
                                            data_col,
                                            shown[ix].clone(),
                                            excluded[ix],
                                            table.clone(),
                                            hover,
                                            muted,
                                        )
                                    })
                                    .collect()
                            },
                        )
                        .track_scroll(&filter_scroll),
                    )
                }),
        )
        .into_any_element()
}

/// One value checkbox. Split out only because the virtual-list callback needs it as a plain
/// `Fn` over an index range — it is not a per-visual-part helper.
fn row(
    data_col: usize,
    value: SharedString,
    is_excluded: bool,
    table: gpui::Entity<TableState<QrateTableDelegate>>,
    hover: gpui::Hsla,
    muted: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    let on_click = {
        let value = value.clone();
        move |_: &bool, _: &mut Window, cx: &mut App| {
            table.update(cx, |s, cx| {
                s.delegate_mut().toggle_filter_value(data_col, &value);
                cx.emit(crate::TableChanged);
                cx.notify();
            });
        }
    };
    // A blank cell is a real, selectable value — give it a visible stand-in so the row isn't an
    // unclickable-looking empty line.
    let blank = value.is_empty();
    let text = if blank {
        SharedString::from("Empty")
    } else {
        value.clone()
    };
    div()
        .id(SharedString::from(format!("filter-row-{value}")))
        .h(px(ROW_H))
        .px_2()
        .flex()
        .items_center()
        .aria_label(text.clone())
        .hover(|r| r.bg(hover))
        .child(
            // Keyed by value, not list position — the search box reorders the list, and an index
            // key would leave hover/focus on the wrong row.
            Checkbox::new(SharedString::from(format!("filter-value-{value}")))
                .checked(!is_excluded)
                // Value as a child, not `.label()`: the label's `line_height(1.0)` clips descenders here.
                .child(
                    div()
                        .w_full()
                        .truncate()
                        // Muted, so the stand-in reads as "this cell is blank" rather than a
                        // literal cell containing "Empty".
                        .when(blank, |t| t.text_color(muted))
                        .child(text),
                )
                .on_click(on_click),
        )
}
