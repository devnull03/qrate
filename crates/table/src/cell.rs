//! Data-cell rendering: plain text plus, while this cell is being edited, an outline marking it as
//! the edit's source and a one-shot capture of its rect (the floating editor itself is rendered by
//! `panel.rs`, outside the virtualized grid, so it survives scrolling). Selection is the table's own
//! native cell selection (`cell_selectable`) — the library draws the active-cell highlight and emits
//! the `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, BorderStyle, Bounds, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div, outline, point, px, size,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, table::TableState};

use diagnostics::Diagnostics;

use crate::EditSpawn;
use crate::note::{self, Target};
use crate::{delegate::QrateTableDelegate, editing::EditState};

/// The library's `Size::Medium` table-cell padding, which our inner cell div sits inside. The
/// captured rect is grown by it so the editor covers the whole cell, not just its text box.
pub(crate) const CELL_PAD_X: f32 = 8.;
pub(crate) const CELL_PAD_Y: f32 = 4.;

/// `row_ix` is a source (not view) row index — `render_td` maps through `visible_rows` first.
/// `col_ix` is a data-column index, not shifted for the pinned row-index column.
pub(crate) fn render_cell(
    delegate: &mut QrateTableDelegate,
    row_ix: usize,
    col_ix: usize,
    _window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let editing = delegate.editing
        == (EditState::Editing {
            row: row_ix,
            col: col_ix,
        });

    // Keep the plain text underneath so the row height and neighbouring cells are unaffected; the
    // editor floats over it.
    let text = delegate.cell(row_ix, col_ix).cloned().unwrap_or_default();
    // Measure once per edit: any later frame would report the cell's *scrolled* position, and the
    // box is meant to stay where the edit opened.
    let capture = editing
        && cx
            .try_global::<EditSpawn>()
            .is_none_or(|s| s.cell != (row_ix, col_ix));
    let accent = cx.theme().primary;

    let location = delegate.location(Some(row_ix), Some(col_ix));
    let worst = Diagnostics::worst_at(
        &location.dataset,
        Some(row_ix),
        location.column.as_deref(),
        cx,
    );
    let tip = note::tooltip_text(&location, cx);
    let note_editor = note::editor(delegate, Some(row_ix), Some(col_ix), cx);

    div()
        // The library ids the *wrapper* cell `table-cell:{r}:{c}`; this is the inner div, and it
        // needs its own id before it can carry a tooltip or a context menu.
        .id(SharedString::from(format!("cell-note:{row_ix}:{col_ix}")))
        .size_full()
        // Own the containing block for the capture canvas below: without this it resolves against
        // the library's cell div, whose padding makes "the bounds" ambiguous.
        .relative()
        .child(text)
        .when_some(worst, |cell, severity| {
            cell.child(note::marker(severity, cx))
        })
        .when_some(tip, |cell, text| {
            cell.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .tooltip_show_delay(note::HOVER_DELAY)
        })
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
        // Outlines the source cell for as long as the edit is open, so it stays findable once the
        // box has been scrolled away from it. Painted rather than bordered because the border would
        // land inside the cell's padding instead of around the cell.
        .when(editing, |cell| {
            cell.child(
                canvas(
                    move |bounds, window, cx| {
                        let bounds = full_cell(bounds);
                        if capture {
                            cx.set_global(EditSpawn {
                                cell: (row_ix, col_ix),
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
                        window.paint_quad(outline(bounds, accent, BorderStyle::Solid));
                    },
                )
                .absolute()
                .size_full(),
            )
        })
        .into_any_element()
}

/// Grow a cell's inner (text) rect back out to the cell's own rect.
fn full_cell(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: bounds.origin - point(px(CELL_PAD_X), px(CELL_PAD_Y)),
        size: bounds.size + size(px(CELL_PAD_X * 2.), px(CELL_PAD_Y * 2.)),
    }
}
