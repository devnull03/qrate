//! Double-click-to-edit: `gpui_component`'s table has no editing support of its own, so a cell
//! becomes editable by swapping its rendered text for a shared inline `InputState` (see
//! `selection.rs`, which does the swap) and committing the typed value back on blur/Enter.

use gpui::{Context, PathPromptOptions, SharedString, Window};
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
    if crate::column_type(delegate, col, cx) == settings::columns::ColumnType::Filename {
        let folder = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|project| {
                project
                    .data
                    .values
                    .get(settings::project::FILES_FOLDER_KEY)
                    .map(|value| value.text())
            })
            .unwrap_or_default();
        let prompt = if folder.is_empty() {
            "Choose the file for this row".into()
        } else {
            format!("Choose a file from {folder}").into()
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt),
        });
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(name) = paths
                    .first()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
            {
                let name: SharedString = name.to_owned().into();
                let _ = cx.update(|_, cx| crate::write_cell(row, col, name, cx));
            }
        })
        .detach();
        return;
    }
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

/// Leave edit mode without writing the typed text anywhere — Escape, the spreadsheet convention.
/// Reports whether there was an edit to abandon, so a caller that finds none can hand the key on
/// rather than swallowing it.
///
/// Dropping the state is the whole cancel: closing the editor moves focus back to the grid, and
/// the blur that follows reaches [`commit`], which finds `Idle` and writes nothing. The editor's
/// abandoned text needs no clearing either — [`seed`] overwrites it when the next edit opens.
pub(crate) fn cancel(delegate: &mut QrateTableDelegate) -> bool {
    !matches!(std::mem::take(&mut delegate.editing), EditState::Idle)
}

/// Write the editor's current text back into whatever is being edited and leave edit mode. No-op
/// if nothing is (e.g. a stray blur, or an edit [`cancel`] has already dropped).
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

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the chained `gpui::*` glob would let gpui's `test` macro shadow
    // the `#[test]` its own expansion emits (see CLAUDE.md).
    use gpui::{AppContext as _, TestAppContext};

    use crate::editing::{self, EditState};
    use crate::{TablePanel, TableStateHandle};

    fn project() -> settings::project::CurrentProject {
        settings::project::CurrentProject {
            file: std::env::temp_dir().join("qrate-editing-cancel.qrate"),
            data: settings::project::ProjectData {
                name: "T".into(),
                columns: vec![settings::project::ProjectColumn {
                    name: "Title".into(),
                    data_type: "Filename".into(),
                    notes: String::new(),
                }],
                headers: vec!["Title".into()],
                rows: vec![vec!["before".into()]],
                row_ids: vec![1],
                values: Default::default(),
            },
        }
    }

    fn panel(
        cx: &mut TestAppContext,
    ) -> gpui::Entity<gpui_component::table::TableState<crate::delegate::QrateTableDelegate>> {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });
        cx.add_window_view(TablePanel::new);
        cx.update(|cx| {
            cx.try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the panel publishes its state handle")
        })
    }

    /// Escape is the one exit from a cell edit that must not write. It cannot stop the blur that
    /// follows, and that blur runs the same commit path Enter does — so dropping the edit state is
    /// what has to make the commit a no-op. The editor is left empty here, which is why a cancel
    /// that failed would show up as a *blanked* cell rather than an unchanged one.
    #[gpui::test]
    fn escape_abandons_the_edit_and_the_blur_after_it_writes_nothing(cx: &mut TestAppContext) {
        let state = panel(cx);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let delegate = state.delegate_mut();
                delegate.editing = EditState::Editing { row: 0, col: 0 };

                assert!(editing::cancel(delegate), "there was an edit to abandon");
                assert_eq!(delegate.editing, EditState::Idle);

                assert!(editing::commit(delegate, cx).is_none());
                assert_eq!(
                    delegate
                        .cell(0, 0)
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                    "before",
                    "the blur after Escape must leave the cell alone"
                );
            });
        });
    }

    #[gpui::test]
    fn a_filename_cell_opens_the_picker_without_opening_the_text_editor(cx: &mut TestAppContext) {
        let state = panel(cx);
        let window = cx.windows()[0];
        cx.update_window(window, |_, window, cx| {
            state.update(cx, |state, cx| {
                editing::start(state.delegate_mut(), 0, 0, window, cx);
                assert_eq!(state.delegate().editing, EditState::Idle);
            });
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
    }

    /// The other half: without the cancel, that same commit does write — so the assertion above is
    /// about `cancel` and not about a commit path that never fires.
    #[gpui::test]
    fn a_commit_with_no_cancel_still_writes_the_editors_text(cx: &mut TestAppContext) {
        let state = panel(cx);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let delegate = state.delegate_mut();
                delegate.editing = EditState::Editing { row: 0, col: 0 };

                assert!(editing::commit(delegate, cx).is_none());
                assert_eq!(
                    delegate
                        .cell(0, 0)
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                    "",
                    "an uncancelled commit writes whatever the editor holds"
                );
            });
        });
    }

    /// Nothing to abandon has to be reported as such: the Escape handler hands the key on when
    /// this is false, and swallowing it there is what used to make Escape a dead key mid-edit.
    #[gpui::test]
    fn cancelling_outside_an_edit_reports_nothing_to_do(cx: &mut TestAppContext) {
        let state = panel(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                assert!(!editing::cancel(state.delegate_mut()));
            });
        });
    }
}
