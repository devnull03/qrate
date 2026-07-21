//! Data-cell rendering: plain text, swapped for the shared inline editor while that cell is
//! being edited (see `editing.rs`). Selection is the table's own native cell selection
//! (`cell_selectable`) — the library draws the active-cell highlight and emits the
//! `TableEvent`s that `TablePanel` bridges (double-click → edit, cursor updates, etc.).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DragMoveEvent,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Size,
    StatefulInteractiveElement as _, Styled as _, Window, deferred, div, px, size,
};
use gpui_component::{ActiveTheme as _, input::Input, table::TableState};
use settings::AppSettings;

use crate::{
    EditorResizeAnchor, delegate::QrateTableDelegate, editing::EditState, floating::clamped_float,
};

/// User-scope ("global") keys for the persisted editor box size, in pixels.
const CELL_EDITOR_W_KEY: &str = "cell_editor_w";
const CELL_EDITOR_H_KEY: &str = "cell_editor_h";

const MIN_W: f32 = 160.;
const MIN_H: f32 = 80.;

/// The persisted editor size (user scope), clamped to sane bounds and never larger than the table.
/// Width defaults to ≈40% of the table so a small window opens a proportionally small box.
fn editor_size(cx: &App, table: Bounds<Pixels>) -> Size<Pixels> {
    let (max_w, max_h) = (table.size.width, table.size.height);
    let read = |k: &str| {
        AppSettings::get(cx)
            .values
            .get(k)
            .and_then(|v| v.text().parse::<f32>().ok())
            .map(px)
    };
    let w = read(CELL_EDITOR_W_KEY)
        .unwrap_or((max_w * 0.4).max(px(MIN_W)))
        .clamp(px(MIN_W).min(max_w), max_w);
    let h = read(CELL_EDITOR_H_KEY)
        .unwrap_or(px(160.))
        .clamp(px(MIN_H).min(max_h), max_h);
    size(w, h)
}

/// Invisible drag preview for the resize handle — the box tracks the cursor via persisted size, so
/// the drag itself renders nothing.
struct ResizeGhost;
impl Render for ResizeGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Marker payload so `on_drag_move` only fires for the editor resize (matched by type).
#[derive(Clone)]
struct EditorResize;

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
    // The table's rect (measured in `panel.rs`) bounds the editor: it caps the box size and is the
    // rect `clamped_float` keeps the box inside.
    let table = cx
        .try_global::<crate::TableViewportBounds>()
        .map(|b| b.0)
        .unwrap_or_default();
    let box_size = editor_size(cx, table);
    let (max_w, max_h) = (table.size.width, table.size.height);
    div()
        .size_full()
        .child(text)
        .when(editing, |cell| {
            // `deferred` paints the box above the grid and lets it escape the cell's clip;
            // `clamped_float` confines it to the *table* rect (not the window), so it can't spill
            // over the details or any other side panel. Dismissed only by the user (Enter/blur
            // commit, Escape, or a filter dropping the row) — never by scrolling.
            cell.child(deferred(clamped_float(
                table,
                div()
                    // Swallow mouse events so clicking/selecting inside the editor doesn't fall
                    // through to the cells painted behind it (moving the table's selection).
                    .occlude()
                    .relative()
                    .w(box_size.width)
                    .h(box_size.height)
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .shadow_lg()
                    .child(Input::new(&delegate.editor).h_full())
                    .child(resize_handle(cx, table, max_w, max_h)),
            )))
        })
        .into_any_element()
}

/// A corner grip the user drags to resize the editor. The chosen size persists in user-scope
/// settings, so every cell opens at the size last set. `AppSettings` debounces the disk write, so
/// updating it on every drag-move is cheap.
fn resize_handle(
    cx: &mut Context<TableState<QrateTableDelegate>>,
    table: Bounds<Pixels>,
    max_w: Pixels,
    max_h: Pixels,
) -> impl IntoElement {
    div()
        .id("cell-editor-resize")
        .absolute()
        .bottom_0()
        .right_0()
        .size(px(14.))
        .cursor(CursorStyle::ResizeUpLeftDownRight)
        // `window.mouse_position()` (absolute) — the `Point` gpui passes here is element-relative,
        // which wouldn't match `on_drag_move`'s absolute positions and would spike the delta.
        .on_drag(EditorResize, move |_, _, window, cx| {
            cx.set_global(EditorResizeAnchor {
                mouse: window.mouse_position(),
                size: editor_size(cx, table),
            });
            cx.new(|_| ResizeGhost)
        })
        .on_drag_move(
            cx.listener(move |_, ev: &DragMoveEvent<EditorResize>, _, cx| {
                let Some((mouse, start)) = cx
                    .try_global::<EditorResizeAnchor>()
                    .map(|a| (a.mouse, a.size))
                else {
                    return;
                };
                let delta = ev.event.position - mouse;
                let w = (start.width + delta.x).clamp(px(MIN_W).min(max_w), max_w);
                let h = (start.height + delta.y).clamp(px(MIN_H).min(max_h), max_h);
                AppSettings::set_text(CELL_EDITOR_W_KEY, format!("{}", f32::from(w)).into(), cx);
                AppSettings::set_text(CELL_EDITOR_H_KEY, format!("{}", f32::from(h)).into(), cx);
            }),
        )
}
