//! Double-click-to-edit: `gpui_component`'s table has no editing support of its own, so a cell
//! becomes editable by swapping its rendered text for a shared inline `InputState` (see
//! `selection.rs`, which does the swap) and committing the typed value back on blur/Enter.

use gpui::{Context, Window};
use gpui_component::table::TableState;

use crate::delegate::QrateTableDelegate;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditState {
    #[default]
    Idle,
    /// `row` is a source-data index, not a view index — the event bridge converts before calling
    /// `start`, so a commit lands on the right row in a filtered view.
    Editing { row: usize, col: usize },
}

/// Seed the shared editor with the cell's current text and enter edit mode. `row` is a source
/// (not view) row index; `col` is a data-column index.
pub(crate) fn start(
    delegate: &mut QrateTableDelegate,
    row: usize,
    col: usize,
    window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) {
    let value = delegate.cell(row, col).cloned().unwrap_or_default();
    let editor = delegate.editor.clone();
    editor.update(cx, |input, cx| input.set_value(value, window, cx));
    editor.update(cx, |input, cx| input.focus(window, cx));
    delegate.editing = EditState::Editing { row, col };
}

/// Write the editor's current text back into the cell being edited and leave edit mode. No-op if
/// nothing is being edited (e.g. a stray blur).
pub(crate) fn commit(
    delegate: &mut QrateTableDelegate,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) {
    let EditState::Editing { row, col } = delegate.editing else {
        return;
    };
    let value = delegate.editor.read(cx).value();
    delegate.set_cell(row, col, value);
    delegate.editing = EditState::Idle;
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
}
