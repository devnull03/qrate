use gpui::*;
use gpui_component::{
    dock::{Panel, PanelControl, PanelEvent},
    input::{InputEvent, InputState},
    table::{DataTable, TableDelegate as _, TableEvent, TableState},
};

use crate::{
    TableStateHandle,
    delegate::{ColumnLayout, QrateTableDelegate, TableChanged},
    editing, row_index,
};

/// Settings key for the saved column layout (order + widths) in the project's `.qrate` file.
const COLUMN_LAYOUT_KEY: &str = "table_columns";

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
    /// Bridges the table's native `TableEvent`s to app behavior: keeps the delegate's selection
    /// cursor, starts edits on double-click, persists the column layout, and re-emits
    /// `TableChanged` so cross-crate readers refresh off one signal.
    _table_sub: Subscription,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| InputState::new(window, cx));
        let mut delegate = QrateTableDelegate::new(editor.clone());
        // Show the open project's data, restoring its saved column order/widths. Without a
        // project (dev launch straight into the main window) the table starts empty.
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            delegate.set_data(&project.data.headers, &project.data.rows);
            Self::restore_columns(&mut delegate, &project.file);
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
                        let cursor = Some((*row, Some(*col - 1)));
                        state.update(cx, |s, _| s.delegate_mut().cursor = cursor);
                    }
                    TableEvent::SelectRow(row) => {
                        let cursor = Some((*row, None));
                        state.update(cx, |s, _| s.delegate_mut().cursor = cursor);
                    }
                    TableEvent::SelectColumn(_) | TableEvent::ClearSelection => {
                        state.update(cx, |s, _| s.delegate_mut().cursor = None);
                    }
                    TableEvent::DoubleClickedCell(row, col) if *col != row_index::COL_IX => {
                        let (row, col) = (*row, *col - 1);
                        state.update(cx, |s, cx| {
                            editing::start(s.delegate_mut(), row, col, window, cx);
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
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &rows);
                    if let Some(layout) =
                        saved.and_then(|json| serde_json::from_str::<ColumnLayout>(&json).ok())
                    {
                        state.delegate_mut().apply_column_layout(&layout);
                    }
                    state.refresh(cx);
                    cx.emit(TableChanged);
                    cx.notify();
                });
                cx.notify();
            });

        Self {
            focus_handle: cx.focus_handle(),
            state,
            _edit_sub,
            _project_sub,
            _table_sub,
        }
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

    // `workspace` embeds this panel with `DockItem::panel`, which has no title bar at all, so
    // this never actually renders — kept `None` regardless, since a zoom control makes no sense
    // for the main body.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for TablePanel {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_2()
            .child(DataTable::new(&self.state).bordered(false))
    }
}
