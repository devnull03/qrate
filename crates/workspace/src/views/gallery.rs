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
use gpui_component::{ActiveTheme, menu::ContextMenuExt as _, table::TableState};
use preview::{can_preview, thumb};
use table::QrateTableDelegate;

/// Card geometry. The width is the slider's business — wide enough to tell two scans of the same
/// document apart at the top of its range, small enough at the bottom that a whole box of prints
/// is on screen at once. Height trails the width by the caption line.
pub(super) const CARD_MIN: f32 = 110.;
pub(super) const CARD_MAX: f32 = 230.;
pub(super) const CARD_DEFAULT: f32 = 168.;
const CAPTION_H: f32 = 16.;
const GAP: f32 = 8.;

/// Project-scoped thumbnail size, in pixels.
pub(super) const THUMB_SIZE_KEY: &str = "gallery_thumb_size";

/// The card width in force, clamped to the slider's range so a hand-edited setting can't produce a
/// wall of one-pixel cards or a single card per screen.
pub(super) fn card_width(cx: &App) -> f32 {
    settings::effective_text(THUMB_SIZE_KEY, cx)
        .parse::<f32>()
        .unwrap_or(CARD_DEFAULT)
        .clamp(CARD_MIN, CARD_MAX)
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
    notes_only: bool,
    cx: &mut App,
) -> AnyElement {
    // Rows the gallery draws: everything visible, or only what has been annotated. Narrowed here
    // rather than through the column filters — "has a note" is not a cell value, and pushing it
    // into `visible_rows` would make the grid and the status bar disagree with the table.
    let rows: Vec<usize> = state.as_ref().map_or_else(Vec::new, |state| {
        let delegate = state.read(cx).delegate();
        delegate
            .visible()
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                !notes_only
                    || diagnostics::Diagnostics::note_count(
                        diagnostics::DATASET_MAIN,
                        Some(**source),
                        None,
                        cx,
                    ) > 0
            })
            .map(|(view, _)| view)
            .collect()
    });
    let count = rows.len();
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

    let card_w = card_width(cx);
    let cols = (f32::from(width) / (card_w + GAP)).floor().max(1.) as usize;
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
                            let slot = band * cols + offset;
                            rows.get(slot).map(|&view| card(&state, view, card_w, cx))
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
    // What the item has been annotated with. The count only — the text lives in Details' Notes
    // sub-panel, so the tile stays a picture rather than becoming a card of prose.
    let notes =
        diagnostics::Diagnostics::note_count(diagnostics::DATASET_MAIN, Some(source), None, cx);

    // Only a decodable image is worth opening; a placeholder card has nothing to zoom into, so
    // clicking one selects the row and leaves the grid up.
    let viewable = path.clone().filter(|path| can_preview(path));

    let state = state.clone();
    div()
        .id(("gallery-card", view))
        .w(px(card_w))
        .h(px(card_w + CAPTION_H))
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .cursor_pointer()
        .rounded(cx.theme().radius)
        // The same pair the grid paints a selected row with, so one item picked in the gallery and
        // the same item picked in the table are recognisably the same state.
        .border_1()
        .border_color(match selected {
            true => cx.theme().table_active_border,
            false => cx.theme().border,
        })
        .when(selected, |card| card.bg(cx.theme().table_active))
        .when(!selected, |card| {
            card.hover(|card| card.bg(cx.theme().accent))
        })
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(thumb(path.as_deref(), preview::CARD, cx))
                .when(pages > 1, |frame| {
                    frame.child(
                        div()
                            .absolute()
                            .top_1()
                            .right_1()
                            .px_1()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().background.opacity(0.75))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{pages} pages")),
                    )
                })
                .when(notes > 0, |frame| {
                    frame.child(
                        div()
                            .absolute()
                            .bottom_1()
                            .right_1()
                            .px_1()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().background.opacity(0.75))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match notes {
                                1 => "1 note".to_string(),
                                n => format!("{n} notes"),
                            }),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .w_full()
                .text_xs()
                .truncate()
                .text_color(cx.theme().muted_foreground)
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
        .on_mouse_down(
            MouseButton::Left,
            move |ev: &MouseDownEvent, _window, cx| {
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
            },
        )
        // Last: `context_menu` wraps the element rather than extending it, so anything chained
        // after this would land on the wrapper instead of the card.
        .context_menu(move |menu, window, cx| {
            table::context_menu(table::MenuTarget::Row(source), menu, window, cx)
        })
        .into_any_element()
}
