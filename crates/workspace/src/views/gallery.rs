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
use gpui_component::{ActiveTheme, table::TableState};
use preview::{can_preview, thumb};
use table::{QrateTableDelegate, Selection};

/// Card geometry. Wide enough to tell two scans of the same document apart, small enough that a
/// batch of thirty is on screen at once.
const CARD_W: f32 = 168.;
const CARD_H: f32 = 184.;
const GAP: f32 = 8.;

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

    let cols = (f32::from(width) / (CARD_W + GAP)).floor().max(1.) as usize;
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
                            (view < count).then(|| card(&state, view, cx))
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
fn card(state: &Entity<TableState<QrateTableDelegate>>, view: usize, cx: &mut App) -> AnyElement {
    let delegate = state.read(cx).delegate();
    let Some(source) = delegate.source(view) else {
        return div().w(px(CARD_W)).into_any_element();
    };
    let path = delegate.row_image(source).map(std::path::Path::to_path_buf);
    let caption = delegate
        .row_fields(source)
        .into_iter()
        .map(|(_, value)| value)
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let selected = matches!(
        delegate.selection(),
        Some(Selection::Cell { row, .. } | Selection::Row(row)) if row == source
    );

    // Only a decodable image is worth opening; a placeholder card has nothing to zoom into, so
    // clicking one selects the row and leaves the grid up.
    let viewable = path.clone().filter(|path| can_preview(path));

    let state = state.clone();
    div()
        .id(("gallery-card", view))
        .w(px(CARD_W))
        .h(px(CARD_H))
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .cursor_pointer()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(match selected {
            true => cx.theme().primary,
            false => cx.theme().border,
        })
        .when(!selected, |card| {
            card.hover(|card| card.bg(cx.theme().accent))
        })
        .child(div().flex_1().min_h_0().overflow_hidden().child(thumb(
            path.as_deref(),
            preview::CARD,
            cx,
        )))
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
        .on_mouse_down(
            MouseButton::Left,
            move |ev: &MouseDownEvent, _window, cx| {
                state.update(cx, |state, cx| state.set_selected_row(view, cx));
                if ev.click_count < 2 {
                    return;
                }
                if let Some(path) = viewable.clone() {
                    crate::image_viewer::open_image_viewer(path, crate::ViewerScope::Centre, cx);
                }
            },
        )
        .into_any_element()
}
