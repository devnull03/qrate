use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Sizable as _,
    button::Button,
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    table::{DataTable, TableDelegate as _, TableEvent, TableState},
};

use crate::{
    TableStateHandle,
    delegate::{ColumnLayout, QrateTableDelegate, Selection, TableChanged, ViewIx},
    editing, photos, row_index,
};

/// Settings key for the saved column layout (order + widths) in the project's `.qrate` file.
const COLUMN_LAYOUT_KEY: &str = "table_columns";

// Free-text find across the grid. Declared here in `crate::table` (not `crate::app`) because the
// dependency edge is app → table: an action declared in app is invisible to `TablePanel`, which
// must handle it. `crates/app/src/actions.rs` binds Ctrl+F to it, scoped to the `TablePanel`
// context so it doesn't steal the shortcut from the cell editor's own `Input` context.
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
    /// Repaints on a user-scope settings change (e.g. the stripe toggle with no project open).
    /// The project-scope case already repaints via `_project_sub`, since writing a project
    /// setting mutates the `CurrentProject` global.
    _settings_sub: Subscription,
    /// Bridges the table's native `TableEvent`s to app behavior: keeps the delegate's selection
    /// cursor, starts edits on double-click, persists the column layout, and re-emits
    /// `TableChanged` so cross-crate readers refresh off one signal.
    _table_sub: Subscription,
    /// The free-text find bar's editor, rendered via `title_suffix` while `search_open`. Its
    /// `Change`/`PressEnter` events drive match recomputation and next/prev navigation.
    search_input: Entity<InputState>,
    /// Whether the find bar is shown. Toggled by `Search` (Ctrl+F / the toolbar button) and
    /// dismissed by Escape.
    search_open: bool,
    /// Current find hits as `(view_row, data_col)` in view order, recomputed on every query
    /// change. Empty when the query is blank or matches nothing.
    search_matches: Vec<(usize, usize)>,
    /// Index into `search_matches` of the hit currently scrolled to / selected.
    search_ix: usize,
    /// Repaints the find bar's "N of M" readout and re-narrows the column-filter dropdown's list
    /// as its search box is typed into.
    _search_sub: Subscription,
    _filter_search_sub: Subscription,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| InputState::new(window, cx));
        let filter_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search values"));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in table"));
        let mut delegate = QrateTableDelegate::new(editor.clone(), filter_search.clone());
        // Show the open project's data, restoring its saved column order/widths. Without a
        // project (dev launch straight into the main window) the table starts empty.
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            delegate.set_data(&project.data.headers, &project.data.rows);
            Self::restore_columns(&mut delegate, &project.file);
            Self::resolve_images(&mut delegate, &project.data);
        }
        let state = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .cell_selectable(true)
                .row_selectable(true)
                .col_selectable(true)
                .col_resizable(true)
                .col_movable(true)
                // Our numbered `#` column (row_index.rs) plays row header, so hide the
                // library's blank strip. With it hidden, clicking the already-selected cell
                // escalates to selecting the whole row.
                .row_header(false)
        });
        // Publish the handle so cross-crate readers (status bar, Details panel) can reach the
        // table; they observe this global to re-bind when the panel is rebuilt.
        cx.set_global(TableStateHandle(state.downgrade()));

        let table_state = state.clone();
        let _edit_sub = cx.subscribe(&editor, move |_this, _editor, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                return;
            }
            table_state.update(cx, |state, cx| {
                editing::commit(state.delegate_mut(), cx);
                cx.emit(TableChanged);
                cx.notify();
            });
        });

        let _table_sub = cx.subscribe_in(
            &state,
            window,
            |_this, state, event: &TableEvent, window, cx| {
                match event {
                    TableEvent::SelectCell(row, col) if *col == row_index::COL_IX => {
                        // Native cell selection ignores per-column `selectable(false)`, so a
                        // click (or arrow-left) can land on the pinned `#` column — bounce to
                        // the first data cell. The nested SelectCell updates the cursor.
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
                            if let Some(source) = s.delegate().source(ViewIx(*row)) {
                                s.delegate_mut().selection =
                                    Some(Selection::Cell { row: source.0, col });
                            }
                        });
                    }
                    TableEvent::SelectRow(row) => {
                        state.update(cx, |s, _| {
                            if let Some(source) = s.delegate().source(ViewIx(*row)) {
                                s.delegate_mut().selection = Some(Selection::Row(source.0));
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
                            if let Some(source) = s.delegate().source(ViewIx(view)) {
                                editing::start(s.delegate_mut(), source.0, col, window, cx);
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
                let (headers, rows) = (project.data.headers.clone(), project.data.rows.clone());
                let saved = settings::project::read_setting(&project.file, COLUMN_LAYOUT_KEY)
                    .ok()
                    .flatten();
                let image_paths =
                    photos::resolve_row_images(&headers, &rows, &Self::files_folder(&project.data));
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &rows);
                    if let Some(layout) =
                        saved.and_then(|json| serde_json::from_str::<ColumnLayout>(&json).ok())
                    {
                        state.delegate_mut().apply_column_layout(&layout);
                    }
                    state.delegate_mut().set_image_paths(image_paths);
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

        // The column-filter dropdown's "search values" box lives on the delegate; repaint when it
        // changes so the open dropdown re-narrows its checklist.
        let _filter_search_sub = cx
            .subscribe(&filter_search, |_this, _input, _event: &InputEvent, cx| {
                cx.notify()
            });

        Self {
            focus_handle: cx.focus_handle(),
            state,
            _edit_sub,
            _project_sub,
            _settings_sub,
            _table_sub,
            search_input,
            search_open: false,
            search_matches: Vec::new(),
            search_ix: 0,
            _search_sub,
            _filter_search_sub,
        }
    }

    /// Recompute the find matches from the current query and jump to the first, if any. Called on
    /// every keystroke in the find bar (the scan is sub-millisecond for qrate's grids).
    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let needle = self.search_input.read(cx).value().to_string();
        self.search_matches = self.state.read(cx).delegate().search_matches(&needle);
        self.search_ix = 0;
        self.select_current_match(cx);
        cx.notify();
    }

    /// Step `delta` matches forward (+1) or back (-1), wrapping, and scroll/select the landing
    /// cell. No-op with no matches.
    fn goto_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.search_matches.len();
        if n == 0 {
            return;
        }
        self.search_ix = (self.search_ix as isize + delta).rem_euclid(n as isize) as usize;
        self.select_current_match(cx);
        cx.notify();
    }

    /// Scroll the current match into view and set it as the native cell selection — reusing the
    /// library's own highlight instead of building span-level match highlighting.
    fn select_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(&(view_row, data_col)) = self.search_matches.get(self.search_ix) else {
            return;
        };
        self.state.update(cx, |state, cx| {
            state.scroll_to_row(view_row, cx);
            // +1 for the pinned row-index column.
            state.set_selected_cell(view_row, data_col + 1, cx);
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
    fn restore_columns(delegate: &mut QrateTableDelegate, file: &std::path::Path) {
        let Ok(Some(json)) = settings::project::read_setting(file, COLUMN_LAYOUT_KEY) else {
            return;
        };
        if let Ok(layout) = serde_json::from_str::<ColumnLayout>(&json) {
            delegate.apply_column_layout(&layout);
        }
    }

    /// The project's persisted files folder (empty if none was linked) — cached settings values
    /// loaded by `settings::project::load_project_file`, no disk I/O here.
    fn files_folder(data: &settings::project::ProjectData) -> String {
        data.values
            .get(settings::project::FILES_FOLDER_KEY)
            .map(|v| v.text().to_string())
            .unwrap_or_default()
    }

    /// Resolves and stores each row's image path against the project's files folder. Re-walks
    /// the folder from disk every call (project open, project switch) — qrate never copies files
    /// in, so this is the only source of truth for where they live right now.
    fn resolve_images(delegate: &mut QrateTableDelegate, data: &settings::project::ProjectData) {
        let folder = Self::files_folder(data);
        let paths = photos::resolve_row_images(&data.headers, &data.rows, &folder);
        delegate.set_image_paths(paths);
    }

    /// Save the current column order + widths into the open project's `.qrate` file
    /// (debounced, off the UI thread). Without a project there's nowhere sensible to put it.
    fn persist_columns(state: &Entity<TableState<QrateTableDelegate>>, cx: &App) {
        let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
            return;
        };
        let layout = state.read(cx).delegate().column_layout();
        let Ok(json) = serde_json::to_string(&layout) else {
            return;
        };
        settings::project::queue_write(&project.file, COLUMN_LAYOUT_KEY, &json, cx);
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
        // Toggle the find bar directly through a weak handle to this panel — more robust than
        // dispatching the `Search` action, which would only reach us if focus happened to sit
        // inside the `TablePanel` context when the button is clicked.
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

    /// Rendered by `TabPanel` immediately left of the toolbar group (the ⋯ cluster). Hosts the
    /// find bar while it's open — `toolbar_buttons` can't, since it's typed `Option<Vec<Button>>`
    /// with no element escape hatch. The bar is a fixed 30px, so the input is `xsmall`.
    fn title_suffix(
        &mut self,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !self.search_open {
            return None;
        }
        let query = self.search_input.read(cx).value();
        let count = if !self.search_matches.is_empty() {
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
        Some(
            h_flex()
                .gap_1()
                .items_center()
                .child(Input::new(&self.search_input).xsmall())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(count),
                ),
        )
    }
}

impl Render for TablePanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stripe = settings::effective_bool(crate::TABLE_STRIPES_KEY, cx);
        div()
            .size_full()
            // `TablePanel` key context so Ctrl+F (bound in `crates/app`) reaches the find toggle
            // without stealing the shortcut from the cell editor's own `Input` context. Tracking
            // the focus handle keeps this node in the focus dispatch path even when no cell holds
            // focus, so the binding fires.
            .key_context("TablePanel")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Search, window, cx| this.toggle_search(window, cx)))
            // The find input propagates Escape (it doesn't consume it), so dismiss the bar here.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.dismiss_search(window, cx)))
            .p_2()
            .child(DataTable::new(&self.state).bordered(false).stripe(stripe))
    }
}
