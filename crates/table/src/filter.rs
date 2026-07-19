//! Google-Sheets-style per-column filter, rendered in the column header via `render_th`. Each
//! header carries a dropdown (`Popover`) with a "search this list" box and a checklist of the
//! column's distinct values; unchecking a value hides its rows. The delegate keeps an *excluded*
//! set per column (`QrateTableDelegate::filters`) — this module is only the UI over it.
//!
//! A `Popover` (not the library's action-based `PopupMenu`) hosts the checklist because it stays
//! open across clicks and can host a focusable input — a `PopupMenu` closes on every item click,
//! so a live checklist/search box can't live in one.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::{
    ActiveTheme, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    popover::Popover,
    table::TableState,
    v_flex,
};

use crate::{TableStateHandle, delegate::QrateTableDelegate, search};

/// Cap on how many value checkboxes the dropdown renders at once — a column with thousands of
/// distinct values would otherwise build thousands of elements per frame. The search box narrows
/// past this; a note shows when the list is clipped.
const MAX_VISIBLE_VALUES: usize = 200;

/// The header cell for a data column: its name plus the filter dropdown trigger.
pub(crate) fn render_th(
    delegate: &QrateTableDelegate,
    data_col: usize,
    _window: &mut Window,
    _cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let name = delegate.column_name(data_col);
    let active = delegate.column_has_filter(data_col);

    h_flex()
        .size_full()
        .justify_between()
        .items_center()
        .gap_1()
        .child(div().flex_1().truncate().child(name))
        .child(
            Popover::new(("col-filter", data_col))
                .trigger(
                    Button::new(("col-filter-btn", data_col))
                        .icon(IconName::ChevronDown)
                        .ghost()
                        .xsmall()
                        // Highlight the affordance while the column is narrowing the view.
                        .selected(active),
                )
                .content(move |_state, window, cx| filter_dropdown(data_col, window, cx)),
        )
        .into_any_element()
}

/// The dropdown body: search box, select-all / clear, and the value checklist. Rebuilt every
/// render (the `Popover` re-invokes this), so it reads live state off the table through the
/// `TableStateHandle` global rather than capturing it.
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
    let (values, filter_search) = {
        let state = table.read(cx);
        let delegate = state.delegate();
        (
            delegate.column_values(data_col),
            delegate.filter_search.clone(),
        )
    };
    let needle = filter_search.read(cx).value().to_lowercase();
    let matched: Vec<SharedString> = values
        .into_iter()
        .filter(|v| search::cell_matches(v.as_ref(), &needle))
        .collect();
    let clipped = matched.len().saturating_sub(MAX_VISIBLE_VALUES);
    let shown: Vec<SharedString> = matched.into_iter().take(MAX_VISIBLE_VALUES).collect();
    let excluded: Vec<bool> = {
        let state = table.read(cx);
        shown
            .iter()
            .map(|v| state.delegate().is_filter_excluded(data_col, v))
            .collect()
    };

    let select_all = {
        let table = table.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            table.update(cx, |s, cx| {
                s.delegate_mut().clear_column_filter(data_col);
                cx.emit(crate::TableChanged);
                cx.notify();
            });
        }
    };
    let clear = {
        let table = table.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            table.update(cx, |s, cx| {
                s.delegate_mut().exclude_all_in_column(data_col);
                cx.emit(crate::TableChanged);
                cx.notify();
            });
        }
    };

    v_flex()
        .w(px(240.))
        .gap_1()
        .p_1()
        .child(Input::new(&filter_search).xsmall())
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new(("filter-all", data_col))
                        .ghost()
                        .xsmall()
                        .label("Select all")
                        .on_click(select_all),
                )
                .child(
                    Button::new(("filter-clear", data_col))
                        .ghost()
                        .xsmall()
                        .label("Clear")
                        .on_click(clear),
                ),
        )
        .child(
            v_flex()
                .id(("filter-values", data_col))
                .max_h(px(240.))
                .overflow_y_scroll()
                .gap_1()
                .children(shown.into_iter().zip(excluded).enumerate().map(
                    |(ix, (value, is_excluded))| {
                        let on_click = {
                            let table = table.clone();
                            let value = value.clone();
                            move |_: &bool, _: &mut Window, cx: &mut App| {
                                table.update(cx, |s, cx| {
                                    s.delegate_mut().toggle_filter_value(data_col, &value);
                                    cx.emit(crate::TableChanged);
                                    cx.notify();
                                });
                            }
                        };
                        Checkbox::new(("filter-value", ix))
                            .checked(!is_excluded)
                            .label(value.clone())
                            .on_click(on_click)
                    },
                )),
        )
        .when(clipped > 0, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("…and {clipped} more (narrow with search)")),
            )
        })
        .into_any_element()
}
