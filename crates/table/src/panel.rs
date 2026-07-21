use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    table::{DataTable, TableDelegate as _, TableEvent, TableState},
    v_flex,
};

use crate::{
    TableStateHandle,
    delegate::{
        ColumnLayout, QrateTableDelegate, SearchOpts, Selection, TableChanged, compile_search,
    },
    editing, photos, row_index,
};

/// Settings key for the saved column layout (order + widths) in the project's `.qrate` file.
const COLUMN_LAYOUT_KEY: &str = "table_columns";

// Free-text find, declared here (not in `app`) since app→table is one-way; `app` binds Ctrl+F to it.
actions!(qrate, [Search]);

/// Center panel: the virtualized text table, with a pinned row-number column, native
/// cell/row/column selection, movable + resizable columns, and double-click-to-edit cells.
pub struct TablePanel {
    focus_handle: FocusHandle,
    state: Entity<TableState<QrateTableDelegate>>,
    /// Commits an in-progress cell edit when the inline editor loses focus or the user presses
    /// Enter. Held alive for as long as the panel exists.
    _edit_sub: Subscription,
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
    /// Repaints the find bar's "N of M" readout and re-narrows the column-filter dropdown's list
    /// as its search box is typed into.
    _search_sub: Subscription,
    _filter_search_sub: Subscription,
    /// Pending debounced autosave (the "timed" mode). Replacing it drops the prior task, which
    /// cancels its timer — that drop *is* the debounce, coalescing a burst of edits into one write.
    _autosave_task: Option<Task<()>>,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Multi-line so long values wrap; the editor fills the user-resizable box (`cell.rs`) and
        // scrolls inside it. `submit_on_enter` keeps Enter as commit (Shift+Enter inserts a newline).
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .submit_on_enter(true)
        });
        let filter_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search values"));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in table"));
        let mut delegate = QrateTableDelegate::new(editor.clone(), filter_search.clone());
        // Show the open project's data, restoring its saved column order/widths. Without a
        // project (dev launch straight into the main window) the table starts empty.
        let mut loaded_project = None;
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            delegate.set_data(&project.data.headers, &project.data.rows);
            Self::apply_saved_layout(&mut delegate, &project.file);
            delegate.set_image_paths(Self::resolve_images(&project.data));
            loaded_project = Some(project.file.clone());
        }
        let column_settings = settings::columns::load(cx);
        let filters_on = settings::columns::filters_master_enabled(cx);
        delegate.apply_column_settings(|key| {
            filters_on && column_settings.get(key).is_some_and(|s| s.filter_enabled)
        });
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
        let _edit_sub = cx.subscribe(&editor, move |this, _editor, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                return;
            }
            table_state.update(cx, |state, cx| {
                editing::commit(state.delegate_mut(), cx);
                cx.emit(TableChanged);
                cx.notify();
            });
            // The commit marked PROJECT_DATA dirty; persist per the Autosave setting.
            this.schedule_autosave(cx);
        });

        let _table_sub = cx.subscribe_in(
            &state,
            window,
            |_this, state, event: &TableEvent, window, cx| {
                match event {
                    TableEvent::SelectCell(row, col) if *col == row_index::COL_IX => {
                        // Native selection ignores `selectable(false)`, so bounce a hit on the `#` column to col 1.
                        let (row, cols) = (*row, state.read(cx).delegate().columns_count(cx));
                        if cols > 1 {
                            state.update(cx, |s, cx| s.set_selected_cell(row, 1, cx));
                        }
                        return;
                    }
                    TableEvent::SelectCell(row, col) => {
                        // `row` is a VIEW index; store the SOURCE row so the selection survives a
                        // filter change and cross-crate readers index the real data.
                        let col = *col - 1;
                        state.update(cx, |s, _| {
                            if let Some(row) = s.delegate().source(*row) {
                                s.delegate_mut().selection = Some(Selection::Cell { row, col });
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
                    let column_settings = settings::columns::load(cx);
                    let filters_on = settings::columns::filters_master_enabled(cx);
                    this.state.update(cx, |state, cx| {
                        state.delegate_mut().apply_column_settings(|key| {
                            filters_on && column_settings.get(key).is_some_and(|s| s.filter_enabled)
                        });
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
                let column_settings = settings::columns::load(cx);
                let filters_on = settings::columns::filters_master_enabled(cx);
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &rows);
                    Self::apply_saved_layout(state.delegate_mut(), &file);
                    state.delegate_mut().set_image_paths(image_paths);
                    state.delegate_mut().apply_column_settings(|key| {
                        filters_on && column_settings.get(key).is_some_and(|s| s.filter_enabled)
                    });
                    state.refresh(cx);
                    cx.emit(TableChanged);
                    cx.notify();
                });
                cx.notify();
            });

        let _settings_sub =
            cx.observe_global::<settings::AppSettings>(|_this: &mut Self, cx| cx.notify());

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

        // Repaint on filter-search change so the dropdown re-narrows; reset scroll to top to avoid a blank box.
        let _filter_search_sub =
            cx.subscribe(&filter_search, |this, _input, _event: &InputEvent, cx| {
                this.state
                    .read(cx)
                    .delegate()
                    .filter_scroll
                    .scroll_to_item(0, ScrollStrategy::Top);
                cx.notify()
            });

        Self {
            focus_handle: cx.focus_handle(),
            state,
            loaded_project,
            _edit_sub,
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
            _filter_search_sub,
            _autosave_task: None,
        }
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

    /// Apply the project's saved column layout onto freshly loaded data, if any.
    fn apply_saved_layout(delegate: &mut QrateTableDelegate, file: &std::path::Path) {
        let Ok(Some(json)) = settings::project::read_setting(file, COLUMN_LAYOUT_KEY) else {
            return;
        };
        if let Ok(layout) = serde_json::from_str::<ColumnLayout>(&json) {
            delegate.apply_column_layout(&layout);
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
    fn persist_columns(state: &Entity<TableState<QrateTableDelegate>>, cx: &mut App) {
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
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            // The find input propagates Escape (it doesn't consume it), so dismiss the bar here.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.dismiss_search(window, cx)))
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
                this.child(
                    h_flex()
                        .flex_none()
                        .gap_1()
                        .items_center()
                        .pb_2()
                        .border_b_1()
                        .border_color(border)
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
                                .on_click(cx.listener(|this, _, _, cx| this.goto_match(-1, cx))),
                        )
                        .child(
                            Button::new("search-next")
                                .icon(IconName::ChevronDown)
                                .ghost()
                                .small()
                                .tooltip("Next match (Enter)")
                                .on_click(cx.listener(|this, _, _, cx| this.goto_match(1, cx))),
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
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
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
                    .child(DataTable::new(&self.state).bordered(false).stripe(stripe)),
            )
    }
}
