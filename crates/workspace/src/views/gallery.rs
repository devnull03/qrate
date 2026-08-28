use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable as _, menu::ContextMenuExt as _, table::TableState,
};
use preview::{can_preview, thumb};
use table::QrateTableDelegate;

pub(super) const COLS_MIN: f32 = 2.;
pub(super) const COLS_MAX: f32 = 12.;
pub(super) const COLS_DEFAULT: f32 = 5.;
const GAP: f32 = 8.;
const PAD: f32 = 8.;

pub(super) const COLUMNS_KEY: &str = "gallery_columns";

pub(super) fn columns(cx: &App) -> usize {
    settings::effective_text(COLUMNS_KEY, cx)
        .parse::<f32>()
        .unwrap_or(COLS_DEFAULT)
        .clamp(COLS_MIN, COLS_MAX) as usize
}

/// Returns `AnyElement`: it is one arm of a `match` in `ViewsPanel::render`, and gpui's chained
/// builder types are deep enough that two concrete ones meeting there overflows rustc's stack.
pub(super) fn render(
    state: Option<Entity<TableState<QrateTableDelegate>>>,
    width: Pixels,
    cols: usize,
    focus: &FocusHandle,
    cx: &mut App,
) -> AnyElement {
    let count = state
        .as_ref()
        .map_or(0, |state| state.read(cx).delegate().visible().len());
    let (Some(state), 1..) = (state, count) else {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No rows to show")
            .into_any_element();
    };

    let card_w = ((f32::from(width) - 2. * PAD - GAP * (cols - 1) as f32) / cols as f32).max(1.);
    let focus = focus.clone();
    uniform_list(
        "gallery",
        count.div_ceil(cols),
        move |range, _window, cx| {
            range
                .map(|band| {
                    div()
                        .flex()
                        .gap(px(GAP))
                        .pb(px(GAP))
                        .children((0..cols).filter_map(|offset| {
                            let view = band * cols + offset;
                            (view < count).then(|| card(&state, view, card_w, &focus, cx))
                        }))
                })
                .collect()
        },
    )
    .size_full()
    .p_2()
    .into_any_element()
}

fn card(
    state: &Entity<TableState<QrateTableDelegate>>,
    view: usize,
    card_w: f32,
    focus: &FocusHandle,
    cx: &mut App,
) -> AnyElement {
    let delegate = state.read(cx).delegate();
    let Some(source) = delegate.source(view) else {
        return div().w(px(card_w)).into_any_element();
    };
    let path = delegate.row_image(source).map(std::path::Path::to_path_buf);
    let caption = delegate
        .row_fields(source)
        .into_iter()
        .map(|(_, value)| value)
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let selected = delegate.is_row_selected(source);
    let pages = path.as_deref().map_or(1, preview::page_count);
    let notes =
        diagnostics::Diagnostics::notes_in_row(diagnostics::DATASET_MAIN, source, cx).count();

    let viewable = path.clone().filter(|path| can_preview(path));

    let radius = cx.theme().radius;
    let badge = move |icon: IconName, count: usize| {
        div()
            .absolute()
            .right_1()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded(radius)
            .bg(black().opacity(0.6))
            .text_xs()
            .text_color(white())
            .child(Icon::new(icon).xsmall().text_color(white()))
            .child(count.to_string())
    };

    let state = state.clone();
    div()
        .id(("gallery-card", view))
        .w(px(card_w))
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .cursor_pointer()
        .rounded(cx.theme().radius)
        .when(selected, |card| card.bg(cx.theme().table_active))
        .child(
            div()
                .relative()
                .h(px(card_w - 8.))
                .overflow_hidden()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .border_1()
                .border_color(cx.theme().border)
                .when(selected, |frame| {
                    frame
                        .border_2()
                        .border_color(cx.theme().table_active_border)
                })
                .when(!selected, |frame| {
                    frame.hover(|frame| frame.border_color(cx.theme().ring))
                })
                .child(thumb(path.as_deref(), preview::CARD, cx))
                .when(pages > 1, |frame| {
                    frame.child(badge(IconName::Copy, pages).top_1())
                })
                .when(notes > 0, |frame| {
                    frame.child(badge(IconName::Menu, notes).bottom_1())
                }),
        )
        .child(
            div()
                .flex_none()
                .w_full()
                .text_xs()
                .truncate()
                .text_color(match selected {
                    true => cx.theme().foreground,
                    false => cx.theme().muted_foreground,
                })
                .child(caption),
        )
        .on_mouse_down(MouseButton::Right, {
            let state = state.clone();
            move |_, _window, cx| {
                state.update(cx, |state, cx| {
                    if !state.delegate().is_row_selected(source) {
                        state.delegate_mut().select_only_row(source);
                        cx.emit(table::TableChanged);
                        cx.notify();
                    }
                });
            }
        })
        .on_mouse_down(MouseButton::Left, {
            let focus = focus.clone();
            move |ev: &MouseDownEvent, window, cx| {
                window.focus(&focus, cx);
                match (ev.modifiers.secondary(), ev.modifiers.shift) {
                    (true, _) => state.update(cx, |state, cx| {
                        state.delegate_mut().toggle_row(source);
                        cx.emit(table::TableChanged);
                        cx.notify();
                    }),
                    (false, true) => state.update(cx, |state, cx| {
                        state.delegate_mut().extend_rows_to(source);
                        cx.emit(table::TableChanged);
                        cx.notify();
                    }),
                    (false, false) => {
                        state.update(cx, |state, cx| {
                            state.delegate_mut().clear_selection();
                            state.set_selected_row(view, cx);
                        });
                    }
                }
                if ev.click_count < 2 {
                    return;
                }
                if let Some(path) = viewable.clone() {
                    crate::viewer::open_viewer(path, crate::ViewerScope::Centre, cx);
                }
            }
        })
        .context_menu(move |menu, window, cx| {
            table::context_menu(table::MenuTarget::Row(source), menu, window, cx)
        })
        .into_any_element()
}
