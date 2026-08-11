use std::ops::RangeInclusive;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    table::{DataTable, TableEvent, TableState},
    v_flex,
};

use plugin_api::{CommandContext, PluginHooks, Suggestions};

use crate::{
    TableStateHandle, cell,
    delegate::{
        ColumnLayout, QrateTableDelegate, SearchOpts, Selection, TableChanged, compile_search,
    },
    editing::{self, EditState},
    floating::float_at,
    history::Cells,
    note, photos, row_index,
};

/// Settings key for the saved column layout (order + widths) in the project's `.qrate` file.
const COLUMN_LAYOUT_KEY: &str = "table_columns";

/// Settings key for how many leading columns are frozen, stored as a decimal count.
pub(crate) const FROZEN_COLUMNS_KEY: &str = "table_frozen_columns";

/// Push the settings the delegate caches into it. Called wherever either store changes, since the
/// delegate reads no settings itself — it has no `App` in the paths that need them.
fn apply_settings(delegate: &mut QrateTableDelegate, cx: &App) {
    let column_settings = settings::columns::load(cx);
    let filters_on = settings::columns::filters_master_enabled(cx);
    delegate.apply_column_settings(
        |key| filters_on && column_settings.get(key).is_some_and(|s| s.filter_enabled),
        settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx),
    );
}

// The grid's own actions, declared here (not in `app`) since app→table is one-way; `app` binds the
// keys and puts Undo/Redo/Cut/Copy/Paste in the Edit menu.
actions!(
    qrate,
    [
        Search,
        Replace,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        Clear,
        EditCell,
        InsertNote,
        UnfreezeColumns,
        InsertRowAbove,
        InsertRowBelow,
        DuplicateRow,
        DeleteRow,
        InsertColumnLeft,
        InsertColumnRight,
        DeleteColumn,
        RenameColumn
    ]
);

/// `gpui_component`'s key context for the grid, which it puts on the table's own focus handle. Our
/// keys bind against this rather than `TablePanel` so they can't fire while the cell editor holds
/// focus — the editor is a sibling of the table, so this context isn't in its dispatch chain.
pub const GRID_CONTEXT: &str = "DataTable";

/// Center panel: the virtualized text table, with a pinned row-number column, native
/// cell/row/column selection, movable + resizable columns, and double-click-to-edit cells.
pub struct TablePanel {
    focus_handle: FocusHandle,
    state: Entity<TableState<QrateTableDelegate>>,
    /// Commits an in-progress cell edit when the inline editor loses focus or the user presses
    /// Enter. Held alive for as long as the panel exists.
    _edit_sub: Subscription,
    /// Saves the open note when its editor loses focus or the user presses Enter.
    _note_sub: Subscription,
    /// Reloads the table when a different project is opened while this window is up.
    _project_sub: Subscription,
    /// Which project's data is currently loaded. `CurrentProject` is mutated by *every*
    /// project-scoped setting write, so `_project_sub` fires far more often than the project
    /// actually changes; without this guard a column-settings toggle would re-run `set_data` and
    /// wipe the user's active filters and selection.
    loaded_project: Option<std::path::PathBuf>,
    /// Repaints on a user-scope settings change (e.g. the stripe toggle with no project open).
    /// The project-scope case already repaints via `_project_sub`, since writing a project
    /// setting mutates the `CurrentProject` global.
    _settings_sub: Subscription,
    /// Bridges the table's native `TableEvent`s to app behavior: keeps the delegate's selection
    /// cursor, starts edits on double-click, persists the column layout, and re-emits
    /// `TableChanged` so cross-crate readers refresh off one signal.
    _table_sub: Subscription,
    /// The free-text find bar's editor, rendered at the top of this panel while `search_open`.
    /// Its `Change`/`PressEnter` events drive match recomputation and next/prev navigation.
    search_input: Entity<InputState>,
    /// Whether the find bar is shown. Toggled by `Search` (Ctrl+F / the toolbar button) and
    /// dismissed by Escape.
    search_open: bool,
    /// Current find hits as `(view_row, data_col)` in view order, recomputed on every query
    /// change. Empty when the query is blank or matches nothing.
    search_matches: Vec<(usize, usize)>,
    /// Index into `search_matches` of the hit currently scrolled to / selected.
    search_ix: usize,
    /// The find bar's match-case / whole-word / regex toggles (Zed's three search options).
    search_opts: SearchOpts,
    /// True when regex mode is on and the query doesn't parse — flips the readout to "Invalid
    /// regex" instead of a misleading "No results".
    search_error: bool,
    /// Repaints the find bar's "N of M" readout as its search box is typed into.
    _search_sub: Subscription,
    /// The replacement text, on the find bar's second row while `replace_open`.
    replace_input: Entity<InputState>,
    /// Whether that second row is shown. Off by default (Zed's chevron): most finds don't replace.
    replace_open: bool,
    /// Replaces the current match on Enter in the replacement box.
    _replace_sub: Subscription,
    /// Pending debounced autosave (the "timed" mode). Replacing it drops the prior task, which
    /// cancels its timer — that drop *is* the debounce, coalescing a burst of edits into one write.
    _autosave_task: Option<Task<()>>,
    /// Pending debounced revalidation, on the same drop-cancels-the-timer trick as the autosave.
    _revalidate_task: Option<Task<()>>,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Multi-line so long values wrap; the box grows to fit them (`cell_editor`) and the input
        // scrolls once it can't grow further. `submit_on_enter` keeps Enter as commit
        // (Shift+Enter inserts a newline).
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .submit_on_enter(true)
        });
        let note_editor = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .submit_on_enter(true)
                .placeholder("Note")
        });
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in table"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
        let mut delegate = QrateTableDelegate::new(editor.clone(), note_editor.clone());
        // Show the open project's data, restoring its saved column order/widths. Without a
        // project (dev launch straight into the main window) the table starts empty.
        let mut loaded_project = None;
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            delegate.set_data(&project.data.headers, &project.data.rows);
            Self::apply_saved_layout(&mut delegate, &project.file);
            delegate.set_image_paths(Self::resolve_images(&project.data));
            loaded_project = Some(project.file.clone());
        }
        apply_settings(&mut delegate, cx);
        // Validator output is never persisted, so opening a project is the only thing that puts it
        // back. Runs before the state entity exists because the delegate already holds the data.
        delegate.revalidate(cx);
        let state = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .cell_selectable(true)
                .row_selectable(true)
                .col_selectable(true)
                .col_resizable(true)
                .col_movable(true)
                // Our `#` column is the row header, so hide the library's blank strip.
                .row_header(false)
        });
        // Publish the handle so cross-crate readers (status bar, Details panel) can reach the
        // table; they observe this global to re-bind when the panel is rebuilt.
        cx.set_global(TableStateHandle(state.downgrade()));

        let table_state = state.clone();
        let _edit_sub = cx.subscribe_in(
            &editor,
            window,
            move |this, _editor, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.ask_for_suggestions(cx);
                    return;
                }
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                // A blur means focus already went where the user clicked; only Enter leaves it
                // homeless.
                let by_enter = matches!(event, InputEvent::PressEnter { .. });
                let rename = table_state.update(cx, |state, cx| {
                    let rename = editing::commit(state.delegate_mut(), cx);
                    cx.emit(TableChanged);
                    cx.notify();
                    rename
                });
                // Outside that `update`: re-keying a column reaches back into this same table.
                if let Some((col, name)) = rename {
                    crate::structural(crate::Structural::RenameColumn { col, name }, cx);
                }
                this.schedule_revalidate(cx);
                this.forget_suggestions(cx);
                // Focus was on the editor, which has just gone away. Hand it back to the grid or
                // the arrow keys and Enter go nowhere — the `DataTable` key context lives on *its*
                // focus handle, not the panel's.
                if by_enter {
                    this.focus_table(window, cx);
                }
                // The commit marked PROJECT_DATA dirty; persist per the Autosave setting.
                this.schedule_autosave(cx);
            },
        );

        // Notes are not `PROJECT_DATA`: `set_note` writes `__notes` itself, so this must not also
        // trip the table autosave (which would rewrite the whole dataset for a typed comment).
        let note_state = state.clone();
        let _note_sub = cx.subscribe(
            &note_editor,
            move |_this, editor, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let message = editor.read(cx).value();
                note_state.update(cx, |state, cx| {
                    let Some(location) = state.delegate_mut().note_edit.take() else {
                        return;
                    };
                    diagnostics::Diagnostics::set_note(location, message, cx);
                    cx.notify();
                });
            },
        );

        let _table_sub = cx.subscribe_in(
            &state,
            window,
            |_this, state, event: &TableEvent, window, cx| {
                match event {
                    TableEvent::SelectCell(row, col) if *col == row_index::COL_IX => {
                        // Native selection ignores `selectable(false)`, so the `#` column reports a
                        // cell hit. Turn it into a whole-row selection — the row header's job, and
                        // the mirror of clicking a column header.
                        let row = *row;
                        state.update(cx, |s, cx| s.set_selected_row(row, cx));
                        return;
                    }
                    TableEvent::SelectCell(view, col) => {
                        // `view` is a VIEW index; store the SOURCE row so the selection survives a
                        // filter change and cross-crate readers index the real data. The range
                        // stays in view coordinates — see `QrateTableDelegate::range`.
                        //
                        // ponytail: shift-click only. Extending with shift+arrow means forking the
                        // library's cell-navigation key handling; do that if anyone asks.
                        let (view, col) = (*view, *col - 1);
                        let extend = window.modifiers().shift;
                        state.update(cx, |s, _| {
                            let delegate = s.delegate_mut();
                            delegate.range = match (extend, delegate.range) {
                                // Grow the existing rectangle from its original anchor.
                                (true, Some((anchor, _))) => Some((anchor, (view, col))),
                                // First shift-click: anchor at wherever the cursor already was.
                                (true, None) => delegate
                                    .selection
                                    .and_then(|s| match s {
                                        Selection::Cell { row, col } => {
                                            Some((delegate.view_row(row)?, col))
                                        }
                                        _ => None,
                                    })
                                    .map(|anchor| (anchor, (view, col))),
                                (false, _) => None,
                            };
                            if let Some(row) = delegate.source(view) {
                                delegate.selection = Some(Selection::Cell { row, col });
                            }
                        });
                    }
                    TableEvent::SelectRow(row) => {
                        state.update(cx, |s, _| {
                            if let Some(row) = s.delegate().source(*row) {
                                s.delegate_mut().selection = Some(Selection::Row(row));
                            }
                        });
                    }
                    // The pinned `#` column isn't a data column, so selecting it clears instead.
                    TableEvent::SelectColumn(col) if *col != row_index::COL_IX => {
                        let sel = Some(Selection::Column(*col - 1));
                        state.update(cx, |s, _| s.delegate_mut().selection = sel);
                    }
                    TableEvent::SelectColumn(_) | TableEvent::ClearSelection => {
                        state.update(cx, |s, _| s.delegate_mut().selection = None);
                    }
                    TableEvent::DoubleClickedCell(row, col) if *col != row_index::COL_IX => {
                        // `row` is a VIEW index; edit the SOURCE row it maps to so the commit
                        // writes back to the correct data row in a filtered view.
                        let (view, col) = (*row, *col - 1);
                        state.update(cx, |s, cx| {
                            if let Some(source) = s.delegate().source(view) {
                                editing::start(s.delegate_mut(), source, col, window, cx);
                            }
                        });
                    }
                    TableEvent::ColumnWidthsChanged(widths) => {
                        state.update(cx, |s, _| s.delegate_mut().set_column_widths(widths));
                        Self::persist_columns(state, cx);
                    }
                    // The delegate's `move_column` hook already reordered the data.
                    TableEvent::MoveColumn(..) => Self::persist_columns(state, cx),
                    _ => {}
                }
                // Cross-crate readers refresh off this single signal.
                state.update(cx, |_, cx| cx.emit(TableChanged));
            },
        );

        let _project_sub =
            cx.observe_global::<settings::project::CurrentProject>(|this: &mut Self, cx| {
                let project = cx.global::<settings::project::CurrentProject>();
                let file = project.file.clone();
                // A project-scoped setting write: re-apply per-column settings only, don't reload (loses state).
                if this.loaded_project.as_ref() == Some(&file) {
                    this.state.update(cx, |state, cx| {
                        apply_settings(state.delegate_mut(), cx);
                        state.refresh(cx);
                        cx.emit(TableChanged);
                        cx.notify();
                    });
                    cx.notify();
                    return;
                }

                let (headers, rows) = (project.data.headers.clone(), project.data.rows.clone());
                let image_paths = Self::resolve_images(&project.data);
                this.loaded_project = Some(file.clone());
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &rows);
                    Self::apply_saved_layout(state.delegate_mut(), &file);
                    state.delegate_mut().set_image_paths(image_paths);
                    apply_settings(state.delegate_mut(), cx);
                    state.refresh(cx);
                    cx.emit(TableChanged);
                    cx.notify();
                });
                cx.notify();
            });

        // User-scope writes land here rather than in the project observer, and the sub-delimiter is
        // one of them — so this has to re-push settings, not just repaint.
        let _settings_sub = cx.observe_global::<settings::AppSettings>(|this: &mut Self, cx| {
            this.state.update(cx, |state, cx| {
                apply_settings(state.delegate_mut(), cx);
                state.refresh(cx);
                cx.emit(TableChanged);
            });
            cx.notify();
        });

        // Typing in the find bar recomputes matches (jumping to the first); Enter/Shift-Enter
        // steps to the next/previous match.
        let _search_sub =
            cx.subscribe(
                &search_input,
                |this, _input, event: &InputEvent, cx| match event {
                    InputEvent::Change => this.refresh_search(cx),
                    InputEvent::PressEnter { shift, .. } => {
                        this.goto_match(if *shift { -1 } else { 1 }, cx)
                    }
                    _ => {}
                },
            );

        let _replace_sub = cx.subscribe(&replace_input, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.replace(false, cx);
            }
        });

        Self {
            focus_handle: cx.focus_handle(),
            state,
            loaded_project,
            _edit_sub,
            _note_sub,
            _project_sub,
            _settings_sub,
            _table_sub,
            search_input,
            search_open: false,
            search_matches: Vec::new(),
            search_ix: 0,
            search_opts: SearchOpts::default(),
            search_error: false,
            _search_sub,
            replace_input,
            replace_open: false,
            _replace_sub,
            _autosave_task: None,
            _revalidate_task: None,
        }
    }

    /// Re-run the validators once the edits stop, rather than inside the commit.
    ///
    /// `Validators::run` re-checks the whole sheet — a 2000-row prose column costs about 6ms of
    /// spell checking on its own, so a sheet with a few of them drops a frame on every committed
    /// edit, paste and undo. Nothing needs squiggles to appear in the same frame as the character
    /// that caused them; every editor makes them a moment late.
    fn schedule_revalidate(&mut self, cx: &mut Context<Self>) {
        self._revalidate_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVALIDATE_DEBOUNCE).await;
            this.update(cx, |_this, cx| crate::revalidate_now(cx)).ok();
        }));
    }

    /// React to a committed cell edit per the Autosave setting: write immediately, buffer behind a
    /// short debounce (the default), or leave it for Ctrl+S / quit. The default and any unset/
    /// unrecognized value both mean "timed".
    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        match settings::effective_text(settings::AUTOSAVE_KEY, cx).as_ref() {
            "off" => {}
            "immediate" => crate::save_now(cx),
            _ => {
                self._autosave_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(800))
                        .await;
                    this.update(cx, |_this, cx| crate::save_now(cx)).ok();
                }));
            }
        }
    }

    /// Recompute the find matches from the current query and jump to the first, if any. Called on
    /// every keystroke in the find bar (the scan is sub-millisecond for qrate's grids).
    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let needle = self.search_input.read(cx).value().to_string();
        self.search_error =
            !needle.trim().is_empty() && compile_search(&needle, self.search_opts).is_none();
        self.search_matches = self
            .state
            .read(cx)
            .delegate()
            .search_matches(&needle, self.search_opts);
        self.search_ix = 0;
        self.select_current_match(cx);
        cx.notify();
    }

    /// Flip one of the three search toggles and re-run the find. `pick` selects which bool.
    fn toggle_opt(&mut self, pick: fn(&mut SearchOpts) -> &mut bool, cx: &mut Context<Self>) {
        let flag = pick(&mut self.search_opts);
        *flag = !*flag;
        self.refresh_search(cx);
    }

    /// Step `delta` matches forward (+1) or back (-1), wrapping, and scroll/select the landing
    /// cell. No-op with no matches.
    fn goto_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        // Re-scan first: matches are view indices, and a filter change since last keystroke invalidates them.
        let needle = self.search_input.read(cx).value().to_string();
        self.search_matches = self
            .state
            .read(cx)
            .delegate()
            .search_matches(&needle, self.search_opts);
        let n = self.search_matches.len();
        if n == 0 {
            return;
        }
        let from = self.search_ix.min(n - 1) as isize;
        self.search_ix = (from + delta).rem_euclid(n as isize) as usize;
        self.select_current_match(cx);
        cx.notify();
    }

    /// Scroll the current match into view and set it as the native cell selection — reusing the
    /// library's own highlight instead of building span-level match highlighting.
    fn select_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(&(view_row, data_col)) = self.search_matches.get(self.search_ix) else {
            return;
        };
        // `set_selected_cell` already centre-scrolls the row, so no separate `scroll_to_row`.
        // +1 for the pinned row-index column.
        self.state.update(cx, |state, cx| {
            state.set_selected_cell(view_row, data_col + 1, cx)
        });
    }

    /// Rewrite the current match's cell, then re-find and land on the next hit. `all` does the
    /// whole visible set instead, as a single undo step. Matches are cell-granular, so this
    /// substitutes every occurrence *within* each affected cell and leaves the rest of its text.
    fn replace(&mut self, all: bool, cx: &mut Context<Self>) {
        let needle = self.search_input.read(cx).value().to_string();
        let replacement = self.replace_input.read(cx).value().to_string();
        let only = match all {
            true => None,
            false => match self.search_matches.get(self.search_ix) {
                Some(&hit) => Some(hit),
                None => return,
            },
        };
        let cells = self.state.read(cx).delegate().replace_edits(
            &needle,
            &replacement,
            self.search_opts,
            only,
        );
        if all {
            log::info!("replace all: rewrote {} cells", cells.len());
        }
        self.write_cells(cells, cx);
        // The rewritten cell stops matching, so everything after it shifts down one — holding the
        // index still is what lands us on the next match (Zed's replace-and-advance).
        let at = self.search_ix;
        self.refresh_search(cx);
        self.search_ix = at.min(self.search_matches.len().saturating_sub(1));
        self.select_current_match(cx);
    }

    /// Toggle the find bar. Opening focuses the query editor; closing returns focus to the table.
    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            self.search_input
                .update(cx, |input, cx| input.focus(window, cx));
            self.refresh_search(cx);
        } else {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    /// Dismiss the find bar (Escape) and return focus to the table. No-op if already closed.
    fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Move focus to the grid itself. `DataTable` binds the arrow keys against its own focus
    /// handle, so anything that takes focus away has to hand it back explicitly.
    fn focus_table(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// Enter opens the selected cell for editing, the spreadsheet convention. Bound in the grid's
    /// own key context, so it can't fire while the editor already has focus and Enter means commit.
    fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            let Some(Selection::Cell { row, col }) = state.delegate().selection() else {
                return;
            };
            editing::start(state.delegate_mut(), row, col, window, cx);
            cx.notify();
        });
    }

    /// The rows and the column an app-menu structural command acts on. The menu bar has no clicked
    /// target the way a right-click does, so the selection is the only answer available — and a
    /// whole-row or whole-column selection only answers one half of the question.
    fn structural_target(&self, cx: &App) -> Option<(Vec<usize>, usize)> {
        let delegate = self.state.read(cx).delegate();
        let (row, col) = match delegate.selection()? {
            Selection::Cell { row, col } => (row, col),
            Selection::Row(row) => (row, 0),
            Selection::Column(col) => (0, col),
        };
        Some((note::target_rows(delegate, row), col))
    }

    /// Run a menu-bar structural command against the selection. `crate::structural` does the rest,
    /// saving included — the menu bar and the right-click menu are the same deliberate click.
    fn structural(
        &mut self,
        pick: impl FnOnce(&[usize], usize) -> crate::Structural,
        cx: &mut Context<Self>,
    ) {
        let Some((rows, col)) = self.structural_target(cx) else {
            log::debug!("no selection — the structural menu command had nothing to act on");
            return;
        };
        crate::structural(pick(&rows, col), cx);
    }

    /// Open the rename editor on the selected column, the menu-bar half of the header's
    /// "Rename column…".
    fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, col)) = self.structural_target(cx) else {
            return;
        };
        self.state.update(cx, |state, cx| {
            editing::start_rename(state.delegate_mut(), col, window, cx);
            cx.notify();
        });
    }

    /// Commit a batch of cell writes as one undoable step, then do everything a committed edit
    /// does. The single path cut, paste and bulk fill all take.
    fn write_cells(&mut self, cells: Cells, cx: &mut Context<Self>) {
        if cells.is_empty() {
            return;
        }
        self.state.update(cx, |state, cx| {
            state.delegate_mut().apply_edit(cells);
            cx.emit(TableChanged);
            cx.notify();
        });
        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
        self.schedule_revalidate(cx);
        self.schedule_autosave(cx);
    }

    /// Put the selected range on the clipboard as TSV — what Sheets and Excel both read and write,
    /// so a range copied here pastes into either. `cut` blanks the range afterwards, as one undo.
    fn copy_range(&mut self, cut: bool, cx: &mut Context<Self>) {
        let Some((rows, cols)) = self.state.read(cx).delegate().range_cells() else {
            return;
        };
        let delegate = self.state.read(cx).delegate();
        let mut lines = Vec::with_capacity(rows.len());
        for &row in &rows {
            let line: Vec<&str> = cols
                .clone()
                .map(|col| delegate.cell(row, col).map_or("", |v| v.as_ref()))
                .collect();
            lines.push(line.join("\t"));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));

        if cut {
            self.clear_range(cx);
        }
    }

    /// Blank every cell in the selection, as one undo step. Cut's second half, and Edit ▸ Clear.
    fn clear_range(&mut self, cx: &mut Context<Self>) {
        let Some((rows, cols)) = self.state.read(cx).delegate().range_cells() else {
            return;
        };
        let mut blanked = Vec::new();
        for row in rows {
            blanked.extend(cols.clone().map(|col| (row, col, SharedString::default())));
        }
        self.write_cells(blanked, cx);
    }

    /// Paste the clipboard over the selection. One clipboard value across a multi-cell selection
    /// fills it (this is the bulk edit); anything else lands as a block at the selection's
    /// top-left, clipped at the grid's edges. Either way it's a single undo step.
    fn paste_range(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let Some((rows, cols)) = self.state.read(cx).delegate().range_cells() else {
            return;
        };
        let block = parse_tsv(&text);
        let reach = self
            .state
            .read(cx)
            .delegate()
            .rows_from(rows[0], block.len());
        if reach.len() < block.len() {
            log::warn!(
                "paste of {} rows clipped to {} — no rows left below the selection",
                block.len(),
                reach.len()
            );
        }
        let cells = paste_cells(&block, &rows, cols, &reach);
        self.write_cells(cells, cx);
    }

    /// Apply the project's saved column layout — order, widths, and how many columns are frozen —
    /// onto freshly loaded data. `set_frozen` clamps, so a stale count from a shrunken dataset is
    /// harmless.
    fn apply_saved_layout(delegate: &mut QrateTableDelegate, file: &std::path::Path) {
        if let Ok(Some(json)) = settings::project::read_setting(file, COLUMN_LAYOUT_KEY)
            && let Ok(layout) = serde_json::from_str::<ColumnLayout>(&json)
        {
            delegate.apply_column_layout(&layout);
        }
        if let Ok(Some(count)) = settings::project::read_setting(file, FROZEN_COLUMNS_KEY)
            && let Ok(count) = count.parse()
        {
            delegate.set_frozen(count);
        }
    }

    /// Each row's image path, resolved against the project's files folder. Re-walks the folder
    /// from disk every call — qrate never copies files in, so disk is the only source of truth.
    fn resolve_images(data: &settings::project::ProjectData) -> Vec<Option<std::path::PathBuf>> {
        let folder = data
            .values
            .get(settings::project::FILES_FOLDER_KEY)
            .map(|v| v.text().to_string())
            .unwrap_or_default();
        photos::resolve_row_images(&data.headers, &data.rows, &folder)
    }

    /// Save the current column order + widths into the open project's `.qrate` file
    /// (debounced, off the UI thread). Without a project there's nowhere sensible to put it.
    pub(crate) fn persist_columns(state: &Entity<TableState<QrateTableDelegate>>, cx: &mut App) {
        let Some(file) = cx
            .try_global::<settings::project::CurrentProject>()
            .map(|p| p.file.clone())
        else {
            return;
        };
        let layout = state.read(cx).delegate().column_layout();
        let Ok(json) = serde_json::to_string(&layout) else {
            return;
        };
        settings::project::queue_write(&file, COLUMN_LAYOUT_KEY, &json, cx);
        settings::dirty::mark(settings::dirty::COLUMN_LAYOUT, cx);
    }

    /// The floating cell editor, sized to its content and pinned to where the edit opened. `None`
    /// unless something is being edited *and* the cell (or header) has measured it, one frame
    /// later.
    fn cell_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let editing = state.delegate().editing;
        let (row, col) = match editing {
            EditState::Idle => return None,
            EditState::Editing { row, col } => (row, col),
            // A header has no row; the label below is the only thing that wants one.
            EditState::Renaming { col } => (0, col),
        };
        let scroll = point(
            state.horizontal_scroll_handle.offset().x,
            state
                .vertical_scroll_handle
                .0
                .borrow()
                .base_handle
                .offset()
                .y,
        );
        let editor = state.delegate().editor.clone();
        let name = state.delegate().column_name(col);
        let label = match editing {
            EditState::Renaming { .. } => format!("Rename {name}"),
            _ => format!("{name} {}", row + 1),
        };
        let spawn = cx.try_global::<crate::EditSpawn>()?;
        if spawn.at != editing {
            return None;
        }
        let cell = spawn.bounds;
        // First render after the measurement, so this *is* the scroll offset it was taken at.
        let spawn_scroll = spawn.scroll.unwrap_or(scroll);
        if spawn.scroll.is_none() {
            cx.global_mut::<crate::EditSpawn>().scroll = Some(scroll);
        }
        let table = cx
            .try_global::<crate::TableViewportBounds>()
            .map(|b| b.0)
            .unwrap_or_default();

        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        // What `Input` actually lays its lines out at (its own `LINE_HEIGHT: Rems(1.25)`), not the
        // window's text style — the two differ and only this one predicts the wrapped height.
        let line_height = window.rem_size() * 1.25;
        let value = editor.read(cx).value();
        // `shape_line` panics on newlines, so hard-wrapped lines are measured one at a time; the
        // widest is what the box would need to show the value unwrapped.
        let natural_w = value.split('\n').fold(px(0.), |widest, line| {
            let w = window
                .text_system()
                .shape_line(
                    SharedString::from(line.to_string()),
                    font_size,
                    &[style.to_run(line.len())],
                    None,
                )
                .width;
            if w > widest { w } else { widest }
        });
        let box_size = editor_size(cell, table, natural_w, line_height, |wrap_w| {
            window
                .text_system()
                .shape_text(
                    value.clone(),
                    font_size,
                    &[style.to_run(value.len())],
                    Some(wrap_w),
                    None,
                )
                .map(|lines| lines.iter().map(|l| 1 + l.wrap_boundaries.len()).sum())
                .unwrap_or(1)
        });

        let scrolled = scroll != spawn_scroll;
        let accent = cx.theme().primary;
        let box_el = div()
            .relative()
            .w(box_size.width)
            .h(box_size.height)
            // Swallow mouse events so clicking inside the box doesn't fall through to the cells
            // painted behind it and move the table's selection.
            .occlude()
            .bg(cx.theme().background)
            .border_1()
            .border_color(accent)
            .rounded(cx.theme().radius)
            .shadow_lg()
            // Once the grid has moved under the box, name the cell it belongs to — the column's
            // own header, which beats a spreadsheet letter when the columns are named.
            .when(scrolled, |b| {
                b.child(
                    div()
                        .absolute()
                        .top(px(-TAB_H))
                        .left_0()
                        .h(px(TAB_H))
                        .px_1()
                        .text_xs()
                        .bg(accent)
                        .text_color(cx.theme().primary_foreground)
                        .rounded_t(cx.theme().radius)
                        .child(label),
                )
            })
            .child(
                Input::new(&editor)
                    .appearance(false)
                    .h_full()
                    .px(px(cell::CELL_PAD_X))
                    .py(px(cell::CELL_PAD_Y))
                    .text_size(font_size),
            )
            .children(self.suggestions(row, col, &value, box_size.height, cx));
        // `cell` is the cell's own rect now that the measuring canvas is pinned to it, so the box
        // opens exactly over what it edits — no correction, and no magic numbers to re-tune.
        Some(deferred(float_at(cell.origin, table, box_el)).into_any_element())
    }

    /// Ask every plugin what could go in the cell being edited, on each keystroke. The host
    /// debounces and drops superseded answers, so this fires as often as the text changes.
    fn ask_for_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(hooks) = cx.try_global::<PluginHooks>().copied() else {
            return;
        };
        let state = self.state.read(cx);
        let EditState::Editing { row, col } = state.delegate().editing else {
            return;
        };
        let delegate = state.delegate();
        let ctx = CommandContext {
            // Left empty: a suggestion request goes to every plugin, so only the host can say which
            // bucket belongs to which of them.
            column_settings: serde_json::Value::Null,
            column: Some(delegate.column_name(col)),
            column_key: Some(delegate.column_key(col)),
            row: Some(row),
            values: Vec::new(),
            argument: Some(delegate.editor.read(cx).value()),
        };
        (hooks.suggest)(&ctx, cx);
    }

    /// The completion list under the editor, drawn from whatever the last request came back with.
    fn suggestions(
        &self,
        row: usize,
        col: usize,
        typed: &SharedString,
        below: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = self.state.read(cx).delegate().column_key(col);
        let offered = cx.try_global::<Suggestions>()?;
        // An answer for the cell the user has since left must not hang over the one they are in.
        if offered.column_key.as_ref() != Some(&key) || offered.row != Some(row) {
            return None;
        }
        let items: Vec<SharedString> = offered
            .items
            .iter()
            .filter(|item| item.as_ref() != typed.as_ref())
            .cloned()
            .collect();
        if items.is_empty() {
            return None;
        }

        let editor = self.state.read(cx).delegate().editor.clone();
        Some(
            div()
                .absolute()
                .top(below + px(2.))
                .left_0()
                .min_w(px(SUGGEST_W))
                .max_h(px(SUGGEST_H))
                .occlude()
                .overflow_hidden()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .shadow_lg()
                .children(items.into_iter().map(|item| {
                    let (editor, chosen) = (editor.clone(), item.clone());
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|row| row.bg(cx.theme().accent))
                        .child(item)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            // Filled in, not committed: the user may still want to add a second
                            // sub-delimited value before leaving the cell.
                            editor.update(cx, |input, cx| {
                                input.set_value(chosen.clone(), window, cx)
                            });
                        })
                }))
                .into_any_element(),
        )
    }

    /// Called when an edit ends, so nothing offered for the cell just left survives into the next
    /// one — including an answer that has not come back yet.
    fn forget_suggestions(&mut self, cx: &mut App) {
        if let Some(hooks) = cx.try_global::<PluginHooks>().copied() {
            (hooks.forget_suggestions)(cx);
        }
    }
}

impl Focusable for TablePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for TablePanel {}

impl Panel for TablePanel {
    fn panel_name(&self) -> &'static str {
        "TablePanel"
    }

    // Main workspace body — no fixed name, so leave the title empty for now.
    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::default()
    }

    /// Center panel: not closable so the main view always keeps its body.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    /// A zoom control makes no sense for the main body. Note this *does* render: `zoomable:
    /// None` only greys the ⋯ menu's "Zoom In" entry and drops the zoom toolbar button — the ⋯
    /// itself is unconditional in `TabPanel::render_toolbar`.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// Rendered by `TabPanel::render_toolbar` immediately left of the ⋯ menu, forced to
    /// `.xsmall().ghost()` by the library. `title_suffix` is the other option, but it sits by the
    /// title instead — this is the hook for buttons that belong *beside* the ⋯.
    fn toolbar_buttons(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> Option<Vec<Button>> {
        // Toggle via a weak handle, not the `Search` action, which only fires when focus is inside `TablePanel`.
        let this = cx.entity().downgrade();
        Some(vec![
            Button::new("table-search")
                .icon(IconName::Search)
                .tooltip("Find in table")
                .on_click(move |_, window, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |panel, cx| panel.toggle_search(window, cx));
                    }
                }),
        ])
    }
}

impl Render for TablePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stripe = settings::effective_bool(crate::TABLE_STRIPES_KEY, cx);
        let query = self.search_input.read(cx).value();
        let count = if self.search_error {
            SharedString::from("Invalid regex")
        } else if !self.search_matches.is_empty() {
            SharedString::from(format!(
                "{} of {}",
                self.search_ix + 1,
                self.search_matches.len()
            ))
        } else if query.trim().is_empty() {
            SharedString::default()
        } else {
            SharedString::from("No results")
        };
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);

        v_flex()
            .size_full()
            // `TablePanel` context + tracked focus so Ctrl+F reaches the toggle even when no cell holds focus.
            .key_context("TablePanel")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Search, window, cx| this.toggle_search(window, cx)))
            // Ctrl+H opens the bar with the replacement row already out, rather than toggling it.
            .on_action(cx.listener(|this, _: &Replace, window, cx| {
                this.replace_open = true;
                if !this.search_open {
                    this.toggle_search(window, cx);
                }
                cx.notify();
            }))
            // The find input propagates Escape (it doesn't consume it), so dismiss the bar here.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.dismiss_search(window, cx)))
            .on_action(cx.listener(|this, _: &EditCell, window, cx| this.edit_selected(window, cx)))
            .on_action(cx.listener(|this, _: &InsertNote, window, cx| {
                note::open_on_selection(&this.state.clone(), window, cx)
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy_range(false, cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.copy_range(true, cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste_range(cx)))
            .on_action(cx.listener(|this, _: &Clear, _, cx| this.clear_range(cx)))
            .on_action(cx.listener(|this, _: &UnfreezeColumns, _, cx| {
                crate::set_frozen_columns(&this.state.clone(), 0, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertRowAbove, _, cx| {
                this.structural(|rows, _| crate::Structural::InsertRow { at: rows[0] }, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertRowBelow, _, cx| {
                this.structural(
                    |rows, _| crate::Structural::InsertRow {
                        at: rows[rows.len() - 1] + 1,
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &DuplicateRow, _, cx| {
                this.structural(
                    |rows, _| crate::Structural::DuplicateRow { row: rows[0] },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &DeleteRow, _, cx| {
                this.structural(|rows, _| crate::Structural::DeleteRows(rows.to_vec()), cx)
            }))
            .on_action(cx.listener(|this, _: &InsertColumnLeft, _, cx| {
                this.structural(|_, col| crate::Structural::InsertColumn { at: col }, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertColumnRight, _, cx| {
                this.structural(|_, col| crate::Structural::InsertColumn { at: col + 1 }, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteColumn, _, cx| {
                this.structural(|_, col| crate::Structural::DeleteColumn { col }, cx)
            }))
            .on_action(
                cx.listener(|this, _: &RenameColumn, window, cx| this.rename_selected(window, cx)),
            )
            .p_2()
            .gap_2()
            // Find bar renders here, not in `title_suffix`: the parent `TabPanel` never observes us to redraw it.
            .when(self.search_open, |this| {
                let opts = self.search_opts;
                // A compact toggle button (case / word / regex), highlighted while active.
                let toggle = |id: &'static str,
                              icon: Option<IconName>,
                              label: &'static str,
                              tip: &'static str,
                              on: bool,
                              pick: fn(&mut SearchOpts) -> &mut bool| {
                    Button::new(id)
                        .ghost()
                        .small()
                        .selected(on)
                        .tooltip(tip)
                        .map(|b| match icon {
                            Some(icon) => b.icon(icon),
                            None => b.label(label),
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_opt(pick, cx)))
                };
                let has_matches = !self.search_matches.is_empty();
                this.child(
                    v_flex()
                        .flex_none()
                        .gap_1()
                        .pb_2()
                        .border_b_1()
                        .border_color(border)
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(
                                    Button::new("replace-toggle")
                                        .ghost()
                                        .small()
                                        .icon(match self.replace_open {
                                            true => IconName::ChevronDown,
                                            false => IconName::ChevronRight,
                                        })
                                        .tooltip("Toggle replace (Ctrl+H)")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.replace_open = !this.replace_open;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .items_center()
                                        .gap_1()
                                        .child(div().flex_1().child(Input::new(&self.search_input)))
                                        .child(toggle(
                                            "search-case",
                                            Some(IconName::CaseSensitive),
                                            "",
                                            "Match case",
                                            opts.case,
                                            |o| &mut o.case,
                                        ))
                                        .child(toggle(
                                            "search-word",
                                            None,
                                            "W",
                                            "Match whole word",
                                            opts.word,
                                            |o| &mut o.word,
                                        ))
                                        .child(toggle(
                                            "search-regex",
                                            None,
                                            ".*",
                                            "Use regular expression",
                                            opts.regex,
                                            |o| &mut o.regex,
                                        )),
                                )
                                .child(
                                    div()
                                        .min_w(px(64.))
                                        .text_xs()
                                        .text_color(muted)
                                        .child(count),
                                )
                                .child(
                                    Button::new("search-prev")
                                        .icon(IconName::ChevronUp)
                                        .ghost()
                                        .small()
                                        .tooltip("Previous match (Shift+Enter)")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.goto_match(-1, cx)),
                                        ),
                                )
                                .child(
                                    Button::new("search-next")
                                        .icon(IconName::ChevronDown)
                                        .ghost()
                                        .small()
                                        .tooltip("Next match (Enter)")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.goto_match(1, cx)),
                                        ),
                                )
                                .child(
                                    Button::new("search-close")
                                        .icon(IconName::Close)
                                        .ghost()
                                        .small()
                                        .tooltip("Close find (Esc)")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.dismiss_search(window, cx)
                                        })),
                                ),
                        )
                        .when(self.replace_open, |bar| {
                            bar.child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    // Indent past the chevron so the two inputs line up.
                                    .pl_7()
                                    .child(div().flex_1().child(Input::new(&self.replace_input)))
                                    .child(
                                        Button::new("replace-one")
                                            .ghost()
                                            .small()
                                            .label("Replace")
                                            .disabled(!has_matches)
                                            .tooltip("Replace in this cell (Enter)")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.replace(false, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("replace-all")
                                            .ghost()
                                            .small()
                                            .label("All")
                                            .disabled(!has_matches)
                                            .tooltip("Replace every match in the visible rows")
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.replace(true, cx)
                                                }),
                                            ),
                                    ),
                            )
                        }),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Record the table area's rect so the floating cell editor can wrap to it and
                    // stay clamped inside it (never over a side panel).
                    .child(
                        canvas(
                            |bounds, _, cx| cx.set_global(crate::TableViewportBounds(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(DataTable::new(&self.state).bordered(false).stripe(stripe))
                    // A sibling of the table, not a child of the edited cell: the grid virtualizes
                    // rows and columns away, and the box has to outlive that.
                    .children(self.cell_editor(window, cx)),
            )
    }
}

/// Clipboard text as a grid. The trailing newline a spreadsheet adds to a copied range is dropped,
/// or it would paste a row of blanks under the real ones.
pub(crate) fn parse_tsv(text: &str) -> Vec<Vec<&str>> {
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .map(|line| {
            line.strip_suffix('\r')
                .unwrap_or(line)
                .split('\t')
                .collect()
        })
        .collect()
}

/// The cells a paste writes. `rows`/`cols` are the selected rectangle (source rows, data columns);
/// `reach` is how far down the block can actually go. Split out of `TablePanel` (which needs a live
/// gpui `App`) so the two cases are unit-testable on plain data.
pub(crate) fn paste_cells(
    block: &[Vec<&str>],
    rows: &[usize],
    cols: RangeInclusive<usize>,
    reach: &[usize],
) -> Cells {
    // One value over a multi-cell selection means "make them all this" — the bulk edit.
    if let [line] = block
        && let [value] = line.as_slice()
        && (rows.len() > 1 || cols.clone().count() > 1)
    {
        let mut cells = Vec::new();
        for &row in rows {
            cells.extend(
                cols.clone()
                    .map(|col| (row, col, SharedString::from(*value))),
            );
        }
        return cells;
    }

    let mut cells = Vec::new();
    for (line, &row) in block.iter().zip(reach) {
        for (offset, value) in line.iter().enumerate() {
            cells.push((row, cols.start() + offset, SharedString::from(*value)));
        }
    }
    // Columns past the last one are dropped by `apply_edit`, which writes only cells that exist.
    cells
}

/// Google-Sheets box sizing. Text that fits gets a cell-sized box; text that overflows keeps the
/// row height and grows rightward to the table's edge; text that still overflows there grows
/// downward to its wrapped height. Never leaves the table rect, so no clamping is needed after.
/// `wrapped_lines` counts display lines at a given wrap width.
fn editor_size(
    cell: Bounds<Pixels>,
    table: Bounds<Pixels>,
    natural_w: Pixels,
    line_height: Pixels,
    wrapped_lines: impl FnOnce(Pixels) -> usize,
) -> Size<Pixels> {
    let avail_w = table.right() - cell.origin.x;
    let avail_h = table.bottom() - cell.origin.y;
    let w = (natural_w + px(PAD_X)).clamp(cell.size.width.min(avail_w), avail_w);
    // Counted at the final width even when the text fits, because a value with hard newlines needs
    // more than one line at any width — assuming otherwise is what put a scrollbar in the box.
    let lines = wrapped_lines(w - px(PAD_X)).max(1);
    let h = (line_height * lines as f32 + px(PAD_Y))
        .max(cell.size.height)
        .min(avail_h);
    size(w, h)
}

/// The editor's own padding — the cell's, so the text sits exactly where it did unedited — plus the
/// 1px border on each side, plus the `RIGHT_MARGIN` a soft-wrapping `Input` subtracts from its own
/// bounds before shaping. Leave that last term out and the box is 10px wider than the width the
/// text was measured at, so the final word wraps into a second line the box has no room for.
const PAD_X: f32 = cell::CELL_PAD_X * 2. + 2. + INPUT_RIGHT_MARGIN;

/// `gpui_component::input::element::RIGHT_MARGIN`, which is private.
const INPUT_RIGHT_MARGIN: f32 = 10.;
const PAD_Y: f32 = cell::CELL_PAD_Y * 2. + 2.;

/// Height of the `C6` tab that marks the box once the grid has scrolled under it.
const TAB_H: f32 = 16.;

/// How long the edits have to stop before the validators re-run. Short enough that squiggles feel
/// immediate, long enough that a burst of commits — a paste, held-down undo — costs one pass.
const REVALIDATE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

/// The completion list under the cell editor. Wide enough for a vocabulary term, short enough that
/// it does not cover the rows a user is comparing against.
const SUGGEST_W: f32 = 220.;
const SUGGEST_H: f32 = 180.;

#[cfg(test)]
mod tests {
    use gpui::{Bounds, Pixels, SharedString, point, px, size};

    /// `paste_cells` output as `(row, col, text)` with plain strs, for readable assertions.
    fn wrote(cells: &[(usize, usize, SharedString)]) -> Vec<(usize, usize, &str)> {
        cells.iter().map(|(r, c, v)| (*r, *c, v.as_ref())).collect()
    }

    #[test]
    fn a_trailing_newline_does_not_paste_a_blank_row() {
        assert_eq!(
            super::parse_tsv("a\tb\nc\td\n"),
            vec![["a", "b"], ["c", "d"]]
        );
        assert_eq!(
            super::parse_tsv("a\tb\r\nc\td"),
            vec![["a", "b"], ["c", "d"]]
        );
    }

    #[test]
    fn one_clipboard_value_fills_a_multi_cell_selection() {
        let block = super::parse_tsv("x");
        let cells = super::paste_cells(&block, &[3, 7], 1..=2, &[3, 7]);
        assert_eq!(
            wrote(&cells),
            vec![(3, 1, "x"), (3, 2, "x"), (7, 1, "x"), (7, 2, "x")]
        );
    }

    /// The same single value over a single cell is an ordinary paste, not a fill.
    #[test]
    fn one_clipboard_value_over_one_cell_writes_only_that_cell() {
        let block = super::parse_tsv("x");
        let cells = super::paste_cells(&block, &[3], 1..=1, &[3]);
        assert_eq!(wrote(&cells), vec![(3, 1, "x")]);
    }

    #[test]
    fn a_block_lands_at_the_selections_top_left_whatever_the_selection_size() {
        let block = super::parse_tsv("a\tb\nc\td");
        let cells = super::paste_cells(&block, &[5], 2..=2, &[5, 6]);
        assert_eq!(
            wrote(&cells),
            vec![(5, 2, "a"), (5, 3, "b"), (6, 2, "c"), (6, 3, "d")]
        );
    }

    /// `reach` is short at the end of the view; the rows that don't exist are simply not written.
    #[test]
    fn a_block_taller_than_the_grid_is_clipped_not_grown() {
        let block = super::parse_tsv("a\nb\nc");
        let cells = super::paste_cells(&block, &[8], 0..=0, &[8, 9]);
        assert_eq!(wrote(&cells), vec![(8, 0, "a"), (9, 0, "b")]);
    }

    /// Rows come from `reach` (view order), so a filtered view pastes down what the user sees
    /// rather than into the source rows hidden between them.
    #[test]
    fn a_block_follows_view_order_across_a_filter() {
        let block = super::parse_tsv("a\nb");
        let cells = super::paste_cells(&block, &[2], 0..=0, &[2, 9]);
        assert_eq!(wrote(&cells), vec![(2, 0, "a"), (9, 0, "b")]);
    }

    /// A 120x32 cell whose top-left is 200px from the table's right edge and 100px from its bottom.
    fn cell_and_table() -> (Bounds<Pixels>, Bounds<Pixels>) {
        let cell = Bounds::new(point(px(300.), px(50.)), size(px(120.), px(32.)));
        let table = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(150.)));
        (cell, table)
    }

    #[test]
    fn text_that_fits_gets_a_cell_sized_box() {
        let (cell, table) = cell_and_table();
        let got = super::editor_size(cell, table, px(40.), px(20.), |_| 1);
        assert_eq!(got, cell.size);
    }

    #[test]
    fn overflowing_text_grows_right_at_the_row_height() {
        let (cell, table) = cell_and_table();
        let got = super::editor_size(cell, table, px(150.), px(20.), |_| 1);
        assert_eq!(got, size(px(150. + super::PAD_X), px(32.)));
    }

    /// A short value with hard newlines fits horizontally but not vertically.
    #[test]
    fn hard_newlines_grow_the_box_down_even_when_the_text_fits() {
        let (cell, table) = cell_and_table();
        let got = super::editor_size(cell, table, px(40.), px(20.), |_| 3);
        assert_eq!(got, size(px(120.), px(60. + super::PAD_Y)));
    }

    #[test]
    fn text_past_the_table_edge_wraps_and_grows_down() {
        let (cell, table) = cell_and_table();
        let got = super::editor_size(cell, table, px(900.), px(20.), |_| 3);
        // Width capped at the table's right edge; height = 3 wrapped lines + padding.
        assert_eq!(got, size(px(200.), px(60. + super::PAD_Y)));
    }

    #[test]
    fn height_never_passes_the_tables_bottom() {
        let (cell, table) = cell_and_table();
        let got = super::editor_size(cell, table, px(900.), px(20.), |_| 40);
        assert_eq!(got.height, px(100.));
    }
}
