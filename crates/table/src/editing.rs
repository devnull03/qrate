//! Double-click-to-edit: `gpui_component`'s table has no editing support of its own, so a cell
//! becomes editable by swapping its rendered text for a shared inline `InputState` (see
//! `selection.rs`, which does the swap) and committing the typed value back on blur/Enter.

use gpui::{Context, SharedString, Window};
use gpui_component::table::TableState;

use crate::delegate::QrateTableDelegate;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditState {
    #[default]
    Idle,
    /// `row` is a source-data index, not a view index — the event bridge converts before calling
    /// `start`, so a commit lands on the right row in a filtered view.
    Editing { row: usize, col: usize },
    /// Typing a column's new name into the same editor, over its header. A rename is an edit of
    /// the one string a column *is*, so it reuses the cell editor rather than growing a third one.
    Renaming { col: usize },
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
    seed(delegate, value, EditState::Editing { row, col }, window, cx);
}

/// [`start`]'s header counterpart: open the editor on a column's name, pre-filled with it.
pub(crate) fn start_rename(
    delegate: &mut QrateTableDelegate,
    col: usize,
    window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) {
    let value = delegate.column_name(col);
    seed(delegate, value, EditState::Renaming { col }, window, cx);
}

fn seed(
    delegate: &mut QrateTableDelegate,
    value: gpui::SharedString,
    state: EditState,
    window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) {
    // Drop the previous edit's measurement, or re-editing the same cell after scrolling would
    // reuse a stale rect (`cell.rs` only re-measures when the edit's identity changes).
    if cx.has_global::<crate::EditSpawn>() {
        cx.remove_global::<crate::EditSpawn>();
    }
    let editor = delegate.editor.clone();
    editor.update(cx, |input, cx| input.set_value(value, window, cx));
    editor.update(cx, |input, cx| input.focus(window, cx));
    delegate.editing = state;
}

/// Write the editor's current text back into whatever is being edited and leave edit mode. No-op
/// if nothing is (e.g. a stray blur).
///
/// A rename comes back for the caller to run instead of being applied here: re-keying a column
/// reaches into the table state this is already inside, and re-entering that `update` would panic.
pub(crate) fn commit(
    delegate: &mut QrateTableDelegate,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> Option<(usize, SharedString)> {
    let value = delegate.editor.read(cx).value();
    let editing = std::mem::take(&mut delegate.editing);
    match editing {
        EditState::Idle => return None,
        EditState::Renaming { col } => return Some((col, value)),
        EditState::Editing { row, col } => delegate.apply_edit(vec![(row, col, value)]),
    }
    settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
    None
}
