//! Gallery view: one thumbnail card per visible row, for the image-heavy collections the grid is
//! the wrong lens on.
//!
//! It owns no data and no selection. Cards come from the delegate's visible rows, so a column
//! filter narrows the gallery exactly as it narrows the grid, and a click goes through
//! `TableState::set_selected_row` — the same call the grid's row header makes — so `TablePanel`'s
//! existing `TableEvent` bridge moves the one shared cursor and the Details panel follows with no
//! wiring of its own.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable as _, menu::ContextMenuExt as _, table::TableState,
};
use preview::{can_preview, thumb};
use table::QrateTableDelegate;

/// Card geometry. The slider sets how many cards go in a row, not how wide one is: the row always
/// spans the view, so cards divide whatever width the panel has between them and there is no
/// leftover gutter to make the grid look off-centre. Height follows the width — a square frame
/// plus the caption line under it.
///
/// Two per row is as coarse as it is worth going; past twelve a card is smaller than the caption
/// under it.
pub(super) const COLS_MIN: f32 = 2.;
pub(super) const COLS_MAX: f32 = 12.;
pub(super) const COLS_DEFAULT: f32 = 5.;
const GAP: f32 = 8.;
const PAD: f32 = 8.;

/// Project-scoped cards per row.
pub(super) const COLUMNS_KEY: &str = "gallery_columns";

/// Cards per row, clamped to the slider's range so a hand-edited setting can't produce a wall of
/// one-pixel cards or a division by zero.
pub(super) fn columns(cx: &App) -> usize {
    settings::effective_text(COLUMNS_KEY, cx)
        .parse::<f32>()
        .unwrap_or(COLS_DEFAULT)
        .clamp(COLS_MIN, COLS_MAX) as usize
}

/// The gallery body. `width` is the measured body width, for the column count.
///
/// A `uniform_list` of card-rows rather than one wrapping scroll container: a wrapping container
/// mounts an element per table row, so a 2000-scan collection — precisely what this view is for —
/// would ask for all 2000 previews on the first frame. This builds only the visible band.
///
/// That bounds what is *requested*; what is *kept* is bounded separately, by the byte budget in
/// `preview`. Both are needed — windowing alone still accumulates every card scrolled past.
///
/// Returns `AnyElement`: it is one arm of a `match` in `ViewsPanel::render`, and gpui's chained
/// builder types are deep enough that two concrete ones meeting there overflows rustc's stack.
pub(super) fn render(
    state: Option<Entity<TableState<QrateTableDelegate>>>,
    width: Pixels,
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

    // The row is the panel minus its padding, split evenly with a gap between each pair. Cards get
    // the width rather than choosing it, so the grid meets both edges at any panel size.
    let cols = columns(cx);
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

/// One row's card: its resolved image (or a placeholder honest about what the file is) over its
/// first non-empty field as a caption, ringed while it holds the selection.
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
    // A scan of a twelve-page ledger and a scan of one photograph look identical at thumbnail
    // size. `page_count` is cheap for everything that isn't a document and cached beyond that.
    let pages = path.as_deref().map_or(1, preview::page_count);
    // What the item has been annotated with, wherever on the row it was filed. The count only —
    // the text lives in Details' Notes sub-panel, so the tile stays a picture rather than becoming
    // a card of prose.
    let notes =
        diagnostics::Diagnostics::notes_in_row(diagnostics::DATASET_MAIN, source, cx).count();

    // Only a decodable image is worth opening; a placeholder card has nothing to zoom into, so
    // clicking one selects the row and leaves the grid up.
    let viewable = path.clone().filter(|path| can_preview(path));

    // A count over the picture, dark enough to read against a blown-out scan and a black one
    // alike — so the corner markers are legible whatever the thumbnail underneath happens to be.
    // Icon plus bare number: at 110px the word "pages" is most of the tile's width.
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
        // The halo behind a ringed frame: the same fill the grid paints a selected row with, so one
        // item picked in the gallery and the same item picked in the table read as one state.
        .when(selected, |card| card.bg(cx.theme().table_active))
        .child(
            // Square, so a wall of cards is a grid rather than a ragged edge — the thumbnail is
            // letterboxed inside it and keeps its own proportions.
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
        // Single click selects and nothing more, so the grid stays browsable: `TablePanel`'s
        // `TableEvent` bridge turns this into `Selection::Row(source)` and the Details panel
        // follows, which is what keeps every view on one cursor. Double click is the deliberate
        // "look at this one" — the photo takes over the centre, leaving the docked panels
        // readable beside it, which is why it opens at `Centre` and not `Workspace`.
        // Right-click selects the card unless it is already part of the selection — the same rule
        // the grid's cells follow, so aiming at a bundle to act on it can't be what destroys it.
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
                // Picking a card puts focus on the centre panel, which is what makes Escape mean
                // "drop this selection" — the cards themselves take no focus of their own.
                window.focus(&focus, cx);
                // The same three gestures the grid answers to, so a selection built in one view is
                // built the same way in the other.
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
        // Last: `context_menu` wraps the element rather than extending it, so anything chained
        // after this would land on the wrapper instead of the card.
        .context_menu(move |menu, window, cx| {
            table::context_menu(table::MenuTarget::Row(source), menu, window, cx)
        })
        .into_any_element()
}
