//! Data-cell rendering: plain text plus, while this cell is being edited, an outline marking it as
//! the edit's source and a one-shot capture of its rect (the floating editor itself is rendered by
//! `panel.rs`, outside the virtualized grid, so it survives scrolling). Selection is the table's own
//! native cell selection (`cell_selectable`) — the library draws the active-cell highlight and emits
//! the `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, BorderStyle, Bounds, Context, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div, fill, outline, point, px, size,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, table::TableState};

use diagnostics::{Diagnostics, Source};

use crate::EditSpawn;
use crate::note::{self, Target};
use crate::{
    delegate::{QrateTableDelegate, Selection},
    editing::EditState,
};

/// The library's `Size::Medium` table-cell padding, which our inner cell div sits inside. The
/// captured rect is grown by it so the editor covers the whole cell, not just its text box.
pub(crate) const CELL_PAD_X: f32 = 8.;
pub(crate) const CELL_PAD_Y: f32 = 4.;

/// `row_ix` is a source (not view) row index — `render_td` maps through `visible_rows` first.
/// `col_ix` is a data-column index, not shifted for the pinned row-index column. `ranged` says the
/// cell is inside the shift-click range; the library paints the one *selected* cell itself, so this
/// only has to tint the rest of the rectangle.
pub(crate) fn render_cell(
    delegate: &mut QrateTableDelegate,
    row_ix: usize,
    col_ix: usize,
    ranged: bool,
    _window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let edit = EditState::Editing {
        row: row_ix,
        col: col_ix,
    };
    let editing = delegate.editing == edit;

    // Keep the plain text underneath so the row height and neighbouring cells are unaffected; the
    // editor floats over it.
    let text = delegate.cell(row_ix, col_ix).cloned().unwrap_or_default();
    // Measure once per edit: any later frame would report the cell's *scrolled* position, and the
    // box is meant to stay where the edit opened.
    let capture = editing && cx.try_global::<EditSpawn>().is_none_or(|s| s.at != edit);
    let accent = cx.theme().primary;
    // The same pair the library paints on the selected cell — alpha-clamped, so the text shows
    // through the fill.
    let (range_bg, range_border) = (cx.theme().table_active, cx.theme().table_active_border);

    let location = delegate.location(Some(row_ix), Some(col_ix));
    // Authored and computed diagnostics get different decorations, as they do in every editor: a
    // note is a corner tag you put there, a validator's finding is a squiggle under the text.
    let worst_from = |authored: bool| {
        Diagnostics::at(
            &location.dataset,
            Some(row_ix),
            location.column.as_deref(),
            cx,
        )
        .filter(move |d| (d.source == Source::Note) == authored)
        .map(|d| d.severity)
        .min()
    };
    let marked = worst_from(true);
    let flagged = worst_from(false);
    let tip = note::tooltip_text(&location, cx);
    let note_editor = note::editor(delegate, Some(row_ix), Some(col_ix), cx);

    div()
        // The library ids the *wrapper* cell `table-cell:{r}:{c}`; this is the inner div, and it
        // needs its own id before it can carry a tooltip or a context menu. `NamedInteger` over a
        // formatted string: the name half is a static `SharedString`, so an id costs no allocation
        // — and this runs for every cell on screen, every frame.
        .id(ElementId::NamedInteger(
            "cell-note".into(),
            (row_ix * delegate.column_count() + col_ix) as u64,
        ))
        .size_full()
        // Own the containing block for the capture canvas below: without this it resolves against
        // the library's cell div, whose padding makes "the bounds" ambiguous.
        .relative()
        .child(text)
        .when_some(marked, |cell, severity| {
            cell.child(note::marker(severity, cx))
        })
        .when_some(flagged, |cell, severity| {
            cell.child(note::squiggle(severity, cx))
        })
        .when_some(tip, |cell, text| {
            cell.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .tooltip_show_delay(note::HOVER_DELAY)
        })
        // Right-click selects, so the cell the menu is about is the cell the menu looks like it is
        // about. A click inside anything already selected — a range, this cell's row, its column —
        // leaves it alone, the way a sheet does: otherwise aiming at a selection to act on it would
        // be what destroys it.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |state, _, _, cx| {
                let Some(view) = state.delegate().view_row(row_ix) else {
                    return;
                };
                let held = matches!(
                    state.delegate().selection(),
                    Some(Selection::Row(r)) if r == row_ix
                ) || matches!(
                    state.delegate().selection(),
                    Some(Selection::Column(c)) if c == col_ix
                );
                if !held && !state.delegate().in_range(view, col_ix) {
                    // Library column indices include the pinned `#` column at 0.
                    state.set_selected_cell(view, col_ix + 1, cx);
                }
            }),
        )
        .context_menu(move |menu, window, cx| {
            note::menu(
                Target::Cell {
                    row: row_ix,
                    col: col_ix,
                },
                menu,
                window,
                cx,
            )
        })
        .when_some(note_editor, |cell, editor| cell.child(editor))
        // Both decorations are painted rather than bordered, because a border would land inside the
        // cell's padding instead of around the cell. The range fill matches what the library draws
        // on the selected cell, so a shift-click rectangle reads as one selection rather than as
        // one selected cell next to some shaded ones.
        .when(editing || ranged, |cell| {
            cell.child(
                canvas(
                    move |bounds, window, cx| {
                        let bounds = full_cell(bounds);
                        if capture {
                            cx.set_global(EditSpawn {
                                at: edit,
                                bounds,
                                scroll: None,
                            });
                            // `panel.rs` only reads this on its *next* render, and nothing else
                            // schedules one. Deferred because `Window::refresh` is a no-op while a
                            // frame is drawing — which is exactly when prepaint runs.
                            window.defer(cx, |window, _| window.refresh());
                        }
                        bounds
                    },
                    move |_, bounds, window, _| {
                        if ranged {
                            window.paint_quad(fill(bounds, range_bg));
                            window.paint_quad(outline(bounds, range_border, BorderStyle::Solid));
                        }
                        // Outlines the source cell for as long as the edit is open, so it stays
                        // findable once the box has scrolled away from it.
                        if editing {
                            window.paint_quad(outline(bounds, accent, BorderStyle::Solid));
                        }
                    },
                )
                // Pinned, not merely `absolute`: with no insets gpui gives an absolute element the
                // static position *after* its in-flow siblings, which put this a whole row below
                // the cell and made every consumer correct for it by hand.
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
        })
        .into_any_element()
}

/// Grow a cell's inner (text) rect back out to the cell's own rect.
pub(crate) fn full_cell(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: bounds.origin - point(px(CELL_PAD_X), px(CELL_PAD_Y)),
        size: bounds.size + size(px(CELL_PAD_X * 2.), px(CELL_PAD_Y * 2.)),
    }
}
