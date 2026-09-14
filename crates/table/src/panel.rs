use std::ops::RangeInclusive;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Escape, Input, InputEvent, InputState, TextareaState},
    slider::{Slider, SliderEvent, SliderState},
    table::{DataTable, TableEvent, TableState},
    v_flex,
};

use plugin_api::{CommandContext, PluginHooks, Suggestions};
use settings::history::Origin;

use crate::{
    TableStateHandle,
    delegate::{
        ColumnLayout, QrateTableDelegate, SearchOpts, Selection, TableChanged, compile_search,
    },
    editing::{self, EditState},
    floating::float_at,
    history::Cells,
    note, photos, row_index, visual,
};

const COLUMN_LAYOUT_KEY: &str = "table_columns";

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
        Deselect,
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
        RenameColumn,
        ExpandAll,
        CollapseAll,
        IndentRow,
        OutdentRow,
        DeleteSubtree
    ]
);

/// `gpui_component`'s key context for the grid, which it puts on the table's own focus handle. Our
/// keys bind against this rather than `TablePanel` so they can't fire while the cell editor holds
/// focus — the editor is a sibling of the table, so this context isn't in its dispatch chain.
pub const GRID_CONTEXT: &str = "DataTable";

pub struct TablePanel {
    focus_handle: FocusHandle,
    state: Entity<TableState<QrateTableDelegate>>,
    _edit_sub: Subscription,
    _note_sub: Subscription,
    _project_sub: Subscription,
    /// Which project's data is currently loaded. `CurrentProject` is mutated by *every*
    /// project-scoped setting write, so `_project_sub` fires far more often than the project
    /// actually changes; without this guard a column-settings toggle would re-run `set_data` and
    /// wipe the user's active filters and selection.
    loaded_project: Option<std::path::PathBuf>,
    _settings_sub: Subscription,
    /// Bridges the table's native `TableEvent`s to app behavior: keeps the delegate's selection
    /// cursor, starts edits on double-click, persists the column layout, and re-emits
    /// `TableChanged` so cross-crate readers refresh off one signal.
    _table_sub: Subscription,
    search_input: Entity<InputState>,
    search_open: bool,
    search_matches: Vec<(usize, usize)>,
    search_ix: usize,
    search_opts: SearchOpts,
    search_error: bool,
    _search_sub: Subscription,
    /// Linked documents being read for a search that includes them. `Some` while it runs.
    reading_documents: Option<Task<()>>,
    /// The pending visual query. Replacing it cancels the one before, which debounces typing.
    visual_query: Option<Task<()>>,
    /// The file a visual search with an empty query ranks against, from "Find similar items".
    similar: Option<std::path::PathBuf>,
    _visual_sub: Subscription,
    /// The last visual ranking as `(score, source_row, data_col)`, best first, so moving the
    /// breadth slider re-cuts it without asking the model again.
    ranking: Vec<(f32, usize, usize)>,
    /// How far below the best visual hit still counts as a match.
    breadth: Entity<SliderState>,
    _breadth_sub: Subscription,
    replace_input: Entity<InputState>,
    replace_open: bool,
    _replace_sub: Subscription,
    /// Pending debounced autosave (the "timed" mode). Replacing it drops the prior task, which
    /// cancels its timer — that drop *is* the debounce, coalescing a burst of edits into one write.
    _autosave_task: Option<Task<()>>,
    /// Pending debounced revalidation, on the same drop-cancels-the-timer trick as the autosave.
    _revalidate_task: Option<Task<()>>,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| TextareaState::new(window, cx).submit_on_enter(true));
        let note_editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .submit_on_enter(true)
                .placeholder("Note")
        });
        crate::editor::configure(&editor, cx);
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in table"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
        let mut delegate = QrateTableDelegate::new(editor.clone(), note_editor.clone());
        let mut loaded_project = None;
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            delegate.set_data(
                &project.data.headers,
                &project.data.row_ids,
                &project.data.rows,
            );
            let structure =
                settings::project::read_row_structure(&project.file).unwrap_or_else(|error| {
                    log::warn!(
                        "Could not read row structure from {:?}: {error}",
                        project.file
                    );
                    Vec::new()
                });
            let description =
                settings::description::DescriptionConfig::from_values(&project.data.values);
            delegate.set_structure(&structure, &description.file_level_key);
            let expanded: Vec<settings::project::RowId> = project
                .data
                .values
                .get(settings::description::HIERARCHY_EXPANDED_KEY)
                .and_then(|value| serde_json::from_str(&value.text()).ok())
                .unwrap_or_default();
            delegate.restore_expanded(&expanded);
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
                .row_header(false)
        });
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
                let by_enter = matches!(event, InputEvent::PressEnter { .. });
                let committed = table_state.update(cx, |state, cx| {
                    let committed = editing::commit(state.delegate_mut(), cx);
                    if matches!(committed, editing::Committed::Cell) {
                        cx.emit(TableChanged);
                        cx.notify();
                    }
                    committed
                });
                match committed {
                    editing::Committed::Unchanged => {}
                    editing::Committed::Cell => {
                        this.schedule_revalidate(cx);
                        this.schedule_autosave(cx);
                    }
                    editing::Committed::Rename(col, name) => {
                        crate::structural(crate::Structural::RenameColumn { col, name }, cx);
                    }
                }
                this.forget_suggestions(cx);
                // Focus was on the editor, which has just gone away. Hand it back to the grid or
                // the arrow keys and Enter go nowhere — the `DataTable` key context lives on *its*
                // focus handle, not the panel's.
                if by_enter {
                    this.focus_table(window, cx);
                }
            },
        );

        // Notes are not `PROJECT_DATA`: `set_note` writes `__notes` itself, so this must not also
        // trip the table autosave (which would rewrite the whole dataset for a typed comment).
        let note_state = state.clone();
        let _note_sub = cx.subscribe_in(
            &note_editor,
            window,
            move |this, editor, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let by_enter = matches!(event, InputEvent::PressEnter { .. });
                let message = editor.read(cx).value();
                note_state.update(cx, |state, cx| {
                    let Some(location) = state.delegate_mut().note_edit.take() else {
                        return;
                    };
                    diagnostics::Diagnostics::set_note(
                        location,
                        message,
                        settings::history::Origin::Typed,
                        cx,
                    );
                    cx.notify();
                });
                if by_enter {
                    this.focus_table(window, cx);
                }
            },
        );

        let _table_sub = cx.subscribe_in(
            &state,
            window,
            |this, state, event: &TableEvent, window, cx| {
                match event {
                    TableEvent::SelectCell(row, col) if *col == row_index::COL_IX => {
                        let view = *row;
                        let modifiers = window.modifiers();
                        if !modifiers.secondary() && !modifiers.shift {
                            state.update(cx, |s, cx| {
                                s.delegate_mut().selected_rows.clear();
                                s.set_selected_row(view, cx);
                            });
                            return;
                        }
                        state.update(cx, |s, _| {
                            let Some(source) = s.delegate().source(view) else {
                                return;
                            };
                            match modifiers.shift {
                                true => s.delegate_mut().extend_rows_to(source),
                                false => s.delegate_mut().toggle_row(source),
                            }
                        });
                    }
                    TableEvent::SelectCell(view, col) => {
                        // `view` is a VIEW index; store the SOURCE row so the selection survives a
                        // filter change and cross-crate readers index the real data. The range
                        // stays in view coordinates — see `QrateTableDelegate::range`.
                        //
                        let (view, col) = (*view, *col - 1);
                        let modifiers = window.modifiers();
                        if modifiers.secondary() {
                            state.update(cx, |s, cx| {
                                if let Some(source) = s.delegate().source(view) {
                                    s.delegate_mut().toggle_row(source);
                                }
                                cx.emit(TableChanged);
                            });
                            return;
                        }
                        let extend = modifiers.shift;
                        state.update(cx, |s, _| {
                            let delegate = s.delegate_mut();
                            if !extend {
                                delegate.selected_rows.clear();
                            }
                            delegate.range = match (extend, delegate.range) {
                                (true, Some((anchor, _))) => Some((anchor, (view, col))),
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
                    TableEvent::MoveColumn(..) => {
                        Self::persist_columns(state, cx);
                        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
                        this.schedule_autosave(cx);
                    }
                    _ => {}
                }
                state.update(cx, |_, cx| cx.emit(TableChanged));
            },
        );

        let _project_sub =
            cx.observe_global::<settings::project::CurrentProject>(|this: &mut Self, cx| {
                let project = cx.global::<settings::project::CurrentProject>();
                let file = project.file.clone();
                if this.loaded_project.as_ref() == Some(&file) {
                    let started = std::time::Instant::now();
                    this.state.update(cx, |state, cx| {
                        apply_settings(state.delegate_mut(), cx);
                        state.refresh(cx);
                        cx.emit(TableChanged);
                        cx.notify();
                    });
                    log::debug!("table re-read project settings in {:?}", started.elapsed());
                    cx.notify();
                    return;
                }
                log::info!("loading project {}", file.display());

                let (headers, row_ids, rows) = (
                    project.data.headers.clone(),
                    project.data.row_ids.clone(),
                    project.data.rows.clone(),
                );
                let image_paths = Self::resolve_images(&project.data);
                let structure =
                    settings::project::read_row_structure(&file).unwrap_or_else(|error| {
                        log::warn!("Could not read row structure from {file:?}: {error}");
                        Vec::new()
                    });
                let description =
                    settings::description::DescriptionConfig::from_values(&project.data.values);
                let expanded: Vec<settings::project::RowId> = project
                    .data
                    .values
                    .get(settings::description::HIERARCHY_EXPANDED_KEY)
                    .and_then(|value| serde_json::from_str(&value.text()).ok())
                    .unwrap_or_default();
                this.loaded_project = Some(file.clone());
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &row_ids, &rows);
                    state
                        .delegate_mut()
                        .set_structure(&structure, &description.file_level_key);
                    state.delegate_mut().restore_expanded(&expanded);
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

        visual::init(cx);
        // Picks up a "find similar" request from a menu, and re-runs a visual search as the model
        // downloads and the index fills in.
        let _visual_sub = cx.observe_global_in::<visual::Visual>(window, |this, window, cx| {
            // Read first: `global_mut` notifies this observer again, so calling it unconditionally loops.
            if cx.global::<visual::Visual>().similar.is_some()
                && let Some(path) = cx.global_mut::<visual::Visual>().similar.take()
            {
                this.similar = Some(path);
                this.search_opts.visual = true;
                this.search_open = true;
                this.search_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                    input.focus(window, cx);
                });
                this.refresh_search(cx);
            } else if this.search_open && this.search_opts.visual {
                this.refresh_search(cx);
            }
        });

        let breadth = cx.new(|_| {
            SliderState::new()
                .min(*visual::BREADTH_RANGE.start())
                .max(*visual::BREADTH_RANGE.end())
                .step(0.01)
                .default_value(visual::BREADTH)
        });
        let _breadth_sub = cx.subscribe(&breadth, |this, _, _: &SliderEvent, cx| {
            this.show_ranking(cx)
        });

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
            reading_documents: None,
            visual_query: None,
            similar: None,
            _visual_sub,
            ranking: Vec::new(),
            breadth,
            _breadth_sub,
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

    fn import_external_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let description = cx
            .try_global::<settings::project::CurrentProject>()
            .map(|project| {
                settings::description::DescriptionConfig::from_values(&project.data.values)
            })
            .unwrap_or_else(|| settings::description::DescriptionProfile::Rad.defaults());
        let folder_level = description.folder_level_key;
        let file_level = description.file_level_key;
        let task = cx.background_executor().spawn(async move {
            file_ingest::plan_paths(
                &paths,
                &file_ingest::PlanOptions {
                    recursive: true,
                    include_root: true,
                    folder_level_key: &folder_level,
                    file_level_key: &file_level,
                },
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(plan) = task.await else {
                log::warn!("the dropped paths could not be inventoried");
                return;
            };
            this.update_in(cx, |_this, window, cx| {
                let files = plan
                    .components
                    .iter()
                    .filter(|component| component.kind == file_ingest::EntryKind::File)
                    .count();
                let folders = plan.components.len() - files;
                let detail = format!(
                    "Add {} component{} ({} file{}, {} folder{})?{}",
                    plan.components.len(),
                    if plan.components.len() == 1 { "" } else { "s" },
                    files,
                    if files == 1 { "" } else { "s" },
                    folders,
                    if folders == 1 { "" } else { "s" },
                    if plan.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" {} path warning(s) will be skipped.", plan.warnings.len())
                    }
                );
                let answer = window.prompt(
                    PromptLevel::Info,
                    "Import dropped files",
                    Some(&detail),
                    &["Import", "Cancel"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await.unwrap_or(1) != 0 {
                        return;
                    }
                    this.update(cx, |this, cx| {
                        let (title_name, file_name) = cx
                            .global::<settings::project::CurrentProject>()
                            .data
                            .columns
                            .iter()
                            .fold((None, None), |(title, file), column| {
                                match settings::columns::ColumnType::from_declared(
                                    &column.data_type,
                                ) {
                                    settings::columns::ColumnType::Title => {
                                        (Some(column.name.clone()), file)
                                    }
                                    settings::columns::ColumnType::Filename => {
                                        (title, Some(column.name.clone()))
                                    }
                                    _ => (title, file),
                                }
                            });
                        this.state.update(cx, |state, cx| {
                            let title_col = title_name
                                .as_deref()
                                .and_then(|name| state.delegate().data_col(name));
                            let file_col = file_name
                                .as_deref()
                                .and_then(|name| state.delegate().data_col(name));
                            state
                                .delegate_mut()
                                .append_components(&plan, title_col, file_col, None);
                            state.refresh(cx);
                            cx.emit(TableChanged);
                            cx.notify();
                        });
                        crate::persist_structure(&this.state, cx);
                        let row_ids = this.state.read(cx).delegate().row_ids().to_vec();
                        diagnostics::Diagnostics::align_note_rows(
                            diagnostics::DATASET_MAIN,
                            &row_ids,
                            cx,
                        );
                        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
                        this.schedule_revalidate(cx);
                        this.schedule_autosave(cx);
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
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
        if self.search_opts.visual {
            self.search_visual(cx);
            return;
        }
        self.ranking.clear();
        let needle = self.search_input.read(cx).value().to_string();
        self.search_error =
            !needle.trim().is_empty() && compile_search(&needle, self.search_opts).is_none();
        // A search that reaches into linked files narrows the view to its hits.
        let hits = (self.search_opts.files && !needle.trim().is_empty() && !self.search_error)
            .then(|| {
                self.state
                    .read(cx)
                    .delegate()
                    .hit_rows(&needle, self.search_opts)
            });
        self.set_search_rows(hits, cx);
        self.search_matches = self
            .state
            .read(cx)
            .delegate()
            .search_matches(&needle, self.search_opts);
        self.search_ix = 0;
        self.select_current_match(cx);
        self.read_documents(cx);
        cx.notify();
    }

    /// Rank rows by how well their linked file matches the query, or looks like the file "Find
    /// similar items" was raised on when the query is empty. Typing waits for a pause first.
    fn search_visual(&mut self, cx: &mut Context<Self>) {
        let files = self.state.read(cx).delegate().linked_files();
        visual::index(files, cx);
        self.search_error = false;
        let needle = self.search_input.read(cx).value().trim().to_string();
        let (query, pause) = match (needle.is_empty(), self.similar.clone()) {
            (false, _) => (
                visual::Query::Text(needle),
                std::time::Duration::from_millis(300),
            ),
            (true, Some(path)) => (visual::Query::Like(path), std::time::Duration::ZERO),
            (true, None) => {
                self.visual_query = None;
                self.ranking.clear();
                self.show_ranking(cx);
                return;
            }
        };
        self.visual_query = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(pause).await;
            let Ok(score) = this.update(cx, |_, cx| visual::scorer(query, cx)) else {
                return;
            };
            let score = score.await;
            this.update(cx, |this, cx| {
                // No scorer means the model is not loaded yet: show every row until it is.
                this.ranking = score
                    .map(|score| this.state.read(cx).delegate().ranked_rows(score))
                    .unwrap_or_default();
                this.show_ranking(cx);
            })
            .ok();
        }));
    }

    /// Narrow the view to the visual ranking's hits within the breadth slider, best first, and step
    /// through them in that order. An empty ranking shows every row.
    fn show_ranking(&mut self, cx: &mut Context<Self>) {
        let breadth = self.breadth.read(cx).value().start();
        let keep = visual::cutoff(self.ranking.iter().map(|hit| hit.0), breadth);
        let hits = &self.ranking[..keep];
        let cols: std::collections::HashMap<usize, usize> =
            hits.iter().map(|&(_, row, col)| (row, col)).collect();
        self.set_search_rows(
            (!hits.is_empty()).then(|| hits.iter().map(|hit| hit.1).collect()),
            cx,
        );
        self.search_matches = self
            .state
            .read(cx)
            .delegate()
            .visible()
            .iter()
            .enumerate()
            .filter_map(|(view, source)| cols.get(source).map(|&col| (view, col)))
            .collect();
        self.search_ix = 0;
        self.select_current_match(cx);
        cx.notify();
    }

    fn set_search_rows(&mut self, rows: Option<Vec<usize>>, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            if state.delegate_mut().set_search_rows(rows) {
                cx.emit(TableChanged);
                cx.notify();
            }
        });
    }

    /// Read the text of every linked document the search has not seen yet, off the UI thread, then
    /// search again so their hits join the results.
    fn read_documents(&mut self, cx: &mut Context<Self>) {
        if !self.search_opts.files || self.reading_documents.is_some() {
            return;
        }
        let unread = self.state.read(cx).delegate().unread_documents();
        if unread.is_empty() {
            return;
        }
        self.reading_documents = Some(cx.spawn(async move |this, cx| {
            let texts = cx
                .background_executor()
                .spawn(async move {
                    unread
                        .into_iter()
                        .map(|path| {
                            let text = preview::document_text(&path).unwrap_or_default();
                            (path, text)
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.reading_documents = None;
                this.state
                    .update(cx, |state, _| state.delegate_mut().add_document_text(texts));
                if this.search_open {
                    this.refresh_search(cx);
                }
            })
            .ok();
        }));
    }

    fn toggle_opt(&mut self, pick: fn(&mut SearchOpts) -> &mut bool, cx: &mut Context<Self>) {
        let flag = pick(&mut self.search_opts);
        *flag = !*flag;
        self.refresh_search(cx);
    }

    fn goto_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        // Re-scan first: matches are view indices, and a filter change since last keystroke invalidates them.
        // A visual ranking costs a model call, so it keeps the order it was given.
        if !self.search_opts.visual {
            let needle = self.search_input.read(cx).value().to_string();
            self.search_matches = self
                .state
                .read(cx)
                .delegate()
                .search_matches(&needle, self.search_opts);
        }
        let n = self.search_matches.len();
        if n == 0 {
            return;
        }
        let from = self.search_ix.min(n - 1) as isize;
        self.search_ix = (from + delta).rem_euclid(n as isize) as usize;
        self.select_current_match(cx);
        cx.notify();
    }

    fn select_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(&(view_row, data_col)) = self.search_matches.get(self.search_ix) else {
            return;
        };
        self.state.update(cx, |state, cx| {
            state.set_selected_cell(view_row, data_col + 1, cx)
        });
    }

    /// Rewrite the current match's cell, then re-find and land on the next hit. `all` does the
    /// whole visible set instead, as a single undo step. Matches are cell-granular, so this
    /// substitutes every occurrence *within* each affected cell and leaves the rest of its text.
    fn replace(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.search_opts.visual {
            return;
        }
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
        self.write_cells(cells, Origin::ReplaceAll, cx);
        // The rewritten cell stops matching, so everything after it shifts down one — holding the
        // index still is what lands us on the next match (Zed's replace-and-advance).
        let at = self.search_ix;
        self.refresh_search(cx);
        self.search_ix = at.min(self.search_matches.len().saturating_sub(1));
        self.select_current_match(cx);
    }

    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            self.search_input
                .update(cx, |input, cx| input.focus(window, cx));
            self.refresh_search(cx);
        } else {
            self.ranking.clear();
            self.set_search_rows(None, cx);
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    /// Open the find bar with its replace row. Replace has nothing to rewrite in a visual search.
    pub fn open_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_opts.visual {
            self.replace_open = true;
        }
        if !self.search_open {
            self.toggle_search(window, cx);
        }
        cx.notify();
    }

    /// Close the find bar if one of its own fields holds focus. `Escape` is bound in the `Input`
    /// context, which the cell editor shares, so the caller must propagate when this declines.
    pub fn escape_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let focused = self.search_input.focus_handle(cx).is_focused(window)
            || self.replace_input.focus_handle(cx).is_focused(window);
        if focused {
            self.dismiss_search(window, cx);
        }
        focused
    }

    fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        self.ranking.clear();
        self.set_search_rows(None, cx);
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Move focus to the grid itself. `DataTable` binds the arrow keys against its own focus
    /// handle, so anything that takes focus away has to hand it back explicitly.
    fn focus_table(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// Abandon the open note before moving focus, so the blur subscription sees no location to
    /// write. Reports whether this Escape belonged to a note editor.
    fn cancel_note_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let cancelled = self.state.update(cx, |state, cx| {
            let cancelled = state.delegate_mut().note_edit.take().is_some();
            if cancelled {
                cx.notify();
            }
            cancelled
        });
        if cancelled {
            self.focus_table(window, cx);
        }
        cancelled
    }

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
    fn write_cells(&mut self, cells: Cells, origin: Origin, cx: &mut Context<Self>) {
        if cells.is_empty() {
            return;
        }
        self.state.update(cx, |state, cx| {
            let files = crate::names_a_file(state.delegate(), &cells, cx);
            state.delegate_mut().apply_edit(cells, origin);
            if files {
                photos::refresh(state, cx);
            }
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
        crate::copy_selection(cx);
        if cut {
            self.clear_range(cx);
        }
    }

    fn clear_range(&mut self, cx: &mut Context<Self>) {
        let Some((rows, cols)) = self.state.read(cx).delegate().range_cells() else {
            return;
        };
        let mut blanked = Vec::new();
        for row in rows {
            blanked.extend(cols.clone().map(|col| (row, col, SharedString::default())));
        }
        self.write_cells(blanked, Origin::Clear, cx);
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
        self.write_cells(cells, Origin::Paste, cx);
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
        photos::resolve_row_images(
            &data.headers,
            &data.rows,
            &folder,
            &photos::declared_file_columns(data),
        )
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

    fn cell_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let editing = state.delegate().editing;
        let (row, col) = match editing {
            EditState::Idle => return None,
            EditState::Editing { row, col } => (row, col),
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

        let value = editor.read(cx).value();
        let (box_el, box_size) =
            crate::editor::editor_box(&editor, label.clone().into(), cell, table, window, cx);

        let scrolled = scroll != spawn_scroll;
        let accent = cx.theme().primary;
        let box_el = box_el
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
            column_settings: serde_json::Value::Null,
            column: Some(delegate.column_name(col)),
            column_key: Some(delegate.column_key(col)),
            row: Some(row),
            values: Vec::new(),
            argument: Some(delegate.editor.read(cx).value()),
        };
        (hooks.suggest)(&ctx, cx);
    }

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

    /// The find bar, or nothing while it is closed. Drawn by whichever view is showing, so a search
    /// carries on across Table and Gallery.
    pub fn render_search_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if !self.search_open {
            return Empty.into_any_element();
        }
        let query = self.search_input.read(cx).value();
        let visual_status = visual::status(cx);
        let count = if let Some(status) = self
            .search_opts
            .visual
            .then(|| visual::status_label(&visual_status))
            .flatten()
        {
            status
        } else if self.search_error {
            SharedString::from("Invalid regex")
        } else if !self.search_matches.is_empty() {
            SharedString::from(format!(
                "{} of {}",
                self.search_ix + 1,
                self.search_matches.len()
            ))
        } else if query.trim().is_empty() {
            match (&self.similar, self.search_opts.visual) {
                (Some(_), true) => SharedString::from("No similar items"),
                _ => SharedString::default(),
            }
        } else if self.reading_documents.is_some() {
            SharedString::from("Reading files…")
        } else {
            SharedString::from("No results")
        };
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        let opts = self.search_opts;
        let toggle = |id: &'static str,
                      icon: Option<Icon>,
                      label: &'static str,
                      tip: &'static str,
                      on: bool,
                      pick: fn(&mut SearchOpts) -> &mut bool| {
            Button::new(id)
                .ghost()
                .small()
                .selected(on)
                // Text rules have no meaning for a query about what an image shows.
                .disabled(opts.visual && id != "search-visual")
                .tooltip(tip)
                .map(|b| match icon {
                    Some(icon) => b.icon(icon),
                    None => b.label(label),
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_opt(pick, cx)))
        };
        let has_matches = !self.search_matches.is_empty();
        v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_2()
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
                            .disabled(opts.visual)
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
                                Some(IconName::CaseSensitive.into()),
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
                            ))
                            .child(toggle(
                                "search-files",
                                Some(IconName::BookOpen.into()),
                                "",
                                "Include text inside linked files",
                                opts.files,
                                |o| &mut o.files,
                            ))
                            .child(toggle(
                                "search-visual",
                                Some(Icon::empty().path("icons/image.svg")),
                                "",
                                "Search by what images show",
                                opts.visual,
                                |o| &mut o.visual,
                            )),
                    )
                    .child(
                        div()
                            .min_w(px(64.))
                            .text_xs()
                            .text_color(muted)
                            .child(count),
                    )
                    .when(opts.visual, |bar| match visual_status {
                        visual::Status::Missing | visual::Status::Failed(_) => bar.child(
                            Button::new("visual-install")
                                .small()
                                .label(visual::download_label())
                                .on_click(|_, _, cx| visual::install(cx)),
                        ),
                        _ => bar.child(
                            div()
                                .id("visual-breadth")
                                .w(px(80.))
                                .tooltip(|window, cx| {
                                    gpui_component::tooltip::Tooltip::new("Fewer or more matches")
                                        .build(window, cx)
                                })
                                .child(Slider::new(&self.breadth)),
                        ),
                    })
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
                            .on_click(
                                cx.listener(|this, _, window, cx| this.dismiss_search(window, cx)),
                            ),
                    ),
            )
            .when(self.replace_open && !opts.visual, |bar| {
                bar.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .pl_7()
                        .child(div().flex_1().child(Input::new(&self.replace_input)))
                        .child(
                            Button::new("replace-one")
                                .ghost()
                                .small()
                                .label("Replace")
                                .disabled(!has_matches)
                                .tooltip("Replace in this cell (Enter)")
                                .on_click(cx.listener(|this, _, _, cx| this.replace(false, cx))),
                        )
                        .child(
                            Button::new("replace-all")
                                .ghost()
                                .small()
                                .label("All")
                                .disabled(!has_matches)
                                .tooltip("Replace every match in the visible rows")
                                .on_click(cx.listener(|this, _, _, cx| this.replace(true, cx))),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Focusable for TablePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TablePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stripe = settings::effective_bool(crate::TABLE_STRIPES_KEY, cx);

        v_flex()
            .size_full()
            .key_context("TablePanel")
            .track_focus(&self.focus_handle)
            .id("table-panel")
            .role(Role::Group)
            .aria_label("Table")
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().secondary_hover))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.import_external_paths(paths.paths().to_vec(), window, cx)
            }))
            // Search, Replace and the find bar's own Escape are handled by `ViewsPanel`, which draws
            // the bar above every view. An action stops propagating by default, so declining an
            // Escape that is not ours has to be explicit.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                // Like a cell edit, a note owns the blur that Escape is about to cause. Drop that
                // ownership first or `_note_sub` will mistake the blur for a commit.
                if this.cancel_note_edit(window, cx) {
                    return;
                }
                // Escape abandons a cell edit (or a column rename) instead of committing it, which
                // is what every spreadsheet does and the only way to back out of a mistyped cell.
                // Enter and clicking away still commit — this is the one exit that does not.
                if this.state.update(cx, |state, cx| {
                    let cancelled = editing::cancel(state.delegate_mut());
                    if cancelled {
                        cx.notify();
                    }
                    cancelled
                }) {
                    // The editor has just gone away; the grid's own keys live on *its* focus
                    // handle, so they need it back.
                    this.focus_table(window, cx);
                } else {
                    cx.propagate();
                }
            }))
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
            .on_action(cx.listener(|this, _: &ExpandAll, _, cx| {
                let expanded = this.state.update(cx, |state, cx| {
                    state.delegate_mut().expand_all();
                    let expanded = state.delegate().expanded_rows();
                    state.refresh(cx);
                    cx.emit(TableChanged);
                    expanded
                });
                crate::persist_expanded(&expanded, cx);
            }))
            .on_action(cx.listener(|this, _: &CollapseAll, _, cx| {
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().collapse_all();
                    state.refresh(cx);
                    cx.emit(TableChanged);
                });
                crate::persist_expanded(&[], cx);
            }))
            .on_action(cx.listener(|this, _: &IndentRow, _, cx| {
                if let Some((rows, _)) = this.structural_target(cx) {
                    crate::arrange(crate::Arrangement::Indent(rows[0]), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OutdentRow, _, cx| {
                if let Some((rows, _)) = this.structural_target(cx) {
                    crate::arrange(crate::Arrangement::Outdent(rows[0]), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DeleteSubtree, _, cx| {
                if let Some((rows, _)) = this.structural_target(cx) {
                    crate::arrange(crate::Arrangement::DeleteSubtree(rows[0]), cx);
                }
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Record the table area's rect so the floating cell editor can wrap to it and
                    // stay clamped inside it (never over a side panel).
                    .child(
                        canvas(
                            |bounds, _, cx| {
                                let measured = crate::TableViewportBounds(bounds);
                                // gpui-kit 0.6 can repaint the virtual table without changing its
                                // viewport. Replacing a global on every prepaint is needless work
                                // on that hot path; only editor placement needs a new value.
                                if cx.try_global::<crate::TableViewportBounds>() != Some(&measured)
                                {
                                    cx.set_global(measured);
                                }
                            },
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
    cells
}

const TAB_H: f32 = 16.;

const REVALIDATE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

const SUGGEST_W: f32 = 220.;
const SUGGEST_H: f32 = 180.;

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use diagnostics::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source};
    use gpui::{SharedString, TestAppContext};
    use settings::history::Origin;

    fn wrote(cells: &[(usize, usize, SharedString)]) -> Vec<(usize, usize, &str)> {
        cells.iter().map(|(r, c, v)| (*r, *c, v.as_ref())).collect()
    }

    fn note_location(row: usize) -> Location {
        Location {
            dataset: DATASET_MAIN.into(),
            row: Some(row),
            row_id: Some(row as i64 + 1),
            column: Some("Title".into()),
        }
    }

    fn project_with_notes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-note-cancel.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: vec!["Title".into()],
                    rows: vec![vec!["one".into()], vec!["two".into()]],
                    row_ids: vec![1, 2],
                    values: Default::default(),
                },
            });
        });
    }

    #[gpui::test]
    fn a_variant_fix_revalidates_and_undo_restores_the_pair(cx: &mut TestAppContext) {
        project_with_notes(cx);
        cx.update(|cx| {
            use gpui::BorrowAppContext as _;
            cx.update_global::<settings::project::CurrentProject, _>(|project, _| {
                project.data.rows = vec![vec!["Agnès Varda".into()], vec!["Varda, Agnès".into()]];
                project.data.values.insert(
                    settings::AUTOSAVE_KEY.into(),
                    settings::Val::Text("off".into()),
                );
                project.data.values.insert(
                    settings::columns::COLUMN_SETTINGS_KEY.into(),
                    settings::Val::Text(r#"{"Title":{"variant_review":true}}"#.into()),
                );
            });
            let variants = clustering::ValueVariants::default();
            diagnostics::Validators::register(Box::new(variants.clone()), cx);
            diagnostics::FixProviders::register(
                clustering::VALUE_VARIANTS_NAME,
                clustering::variant_fixes,
                cx,
            );
            cx.set_global(variants);
        });
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);
        cx.run_until_parked();
        let group = panel.update(cx, |_, cx| {
            assert_eq!(Diagnostics::all(cx).len(), 2);
            let location = Location::cell(DATASET_MAIN, 0, None, "Title");
            let fix = clustering::variant_fixes(&location, "Agnès Varda", None, cx).remove(0);
            crate::write_cell(
                0,
                0,
                fix.replacement,
                Origin::Fix(fix.label.to_string()),
                cx,
            );
            Diagnostics::all(cx)[0].group.clone()
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert!(Diagnostics::all(cx).is_empty());
            assert_eq!(
                panel.state.read(cx).delegate().cell(1, 0).unwrap(),
                "Varda, Agnès"
            );
            panel.state.update(cx, |state, _| {
                assert!(
                    state
                        .delegate_mut()
                        .undo()
                        .is_some_and(|c| !crate::history::moves_rows(&c))
                );
                assert_eq!(
                    state.delegate_mut().undo(),
                    None,
                    "one fix is one undo step"
                );
            });
            crate::revalidate_now(cx);
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert_eq!(Diagnostics::all(cx).len(), 2);
            assert_eq!(Diagnostics::all(cx)[0].group, group);
            assert_eq!(
                panel.state.read(cx).delegate().cell(0, 0).unwrap(),
                "Agnès Varda"
            );
        });
    }

    #[gpui::test]
    fn grouped_fixes_write_every_cell_in_one_undo_step(cx: &mut TestAppContext) {
        project_with_notes(cx);
        cx.update(|cx| {
            use gpui::BorrowAppContext as _;
            cx.update_global::<settings::project::CurrentProject, _>(|project, _| {
                project.data.values.insert(
                    settings::AUTOSAVE_KEY.into(),
                    settings::Val::Text("off".into()),
                );
            });
        });
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);
        panel.update(cx, |panel, cx| {
            crate::set_cell_texts(
                vec![
                    (
                        Location::cell(DATASET_MAIN, 0, None, "Title"),
                        "fixed".into(),
                    ),
                    (
                        Location::cell(DATASET_MAIN, 1, None, "Title"),
                        "fixed".into(),
                    ),
                ],
                Origin::Fix("Use fixed".into()),
                cx,
            );
            assert_eq!(panel.state.read(cx).delegate().cell(0, 0).unwrap(), "fixed");
            assert_eq!(panel.state.read(cx).delegate().cell(1, 0).unwrap(), "fixed");
            panel.state.update(cx, |state, _| {
                assert!(
                    state
                        .delegate_mut()
                        .undo()
                        .is_some_and(|c| !crate::history::moves_rows(&c))
                );
                assert_eq!(state.delegate_mut().undo(), None);
                assert_eq!(state.delegate().cell(0, 0).unwrap(), "one");
                assert_eq!(state.delegate().cell(1, 0).unwrap(), "two");
            });
        });
    }

    /// Escape must clear the note's target before returning focus to the grid. That focus move
    /// inevitably blurs the input; if the target survived, the blur subscription would overwrite
    /// the existing note or create the new draft.
    #[gpui::test]
    fn escape_abandons_existing_and_new_note_drafts(cx: &mut TestAppContext) {
        project_with_notes(cx);
        let existing = note_location(0);
        cx.update(|cx| {
            Diagnostics::set(
                &Source::Note,
                DATASET_MAIN,
                vec![Diagnostic {
                    location: existing.clone(),
                    severity: Severity::Note,
                    source: Source::Note,
                    message: "original".into(),
                    group: None,
                    filed: None,
                }],
                cx,
            );
        });
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);

        for (location, seed) in [(existing.clone(), "original"), (note_location(1), "")] {
            panel.update_in(cx, |panel, window, cx| {
                panel.state.update(cx, |state, cx| {
                    let editor = state.delegate().note_editor.clone();
                    editor.update(cx, |input, cx| {
                        input.set_value(seed, window, cx);
                        input.focus(window, cx);
                        input.set_value("draft", window, cx);
                    });
                    state.delegate_mut().note_edit = Some(location);
                    cx.notify();
                });
                assert!(panel.cancel_note_edit(window, cx));
                let editor = panel.state.read(cx).delegate().note_editor.clone();
                editor.update(cx, |_input, cx| {
                    cx.emit(gpui_component::input::InputEvent::Blur)
                });
            });
            cx.run_until_parked();
        }

        cx.update(|_, cx| {
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(0), Some("Title"), cx).as_deref(),
                Some("original")
            );
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(1), Some("Title"), cx),
                None
            );
        });
    }

    /// Cancellation is the exceptional exit: moving focus normally still commits the note.
    #[gpui::test]
    fn an_ordinary_note_blur_still_commits(cx: &mut TestAppContext) {
        project_with_notes(cx);
        let location = note_location(0);
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);
        panel.update_in(cx, |panel, window, cx| {
            panel.state.update(cx, |state, cx| {
                let editor = state.delegate().note_editor.clone();
                editor.update(cx, |input, cx| {
                    input.set_value("committed", window, cx);
                    input.focus(window, cx);
                });
                state.delegate_mut().note_edit = Some(location);
                cx.notify();
            });
            let editor = panel.state.read(cx).delegate().note_editor.clone();
            editor.update(cx, |_input, cx| {
                cx.emit(gpui_component::input::InputEvent::Blur)
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            assert_eq!(
                Diagnostics::note_at(DATASET_MAIN, Some(0), Some("Title"), cx).as_deref(),
                Some("committed")
            );
        });
    }

    /// What Backspace and Delete do to the selection, and the promise that it is one undo step.
    /// Autosave off so the temp project file is never written.
    #[gpui::test]
    fn clearing_the_selection_blanks_it_and_undoes_as_one_step(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let mut app = settings::AppSettings::default();
            app.values.insert(
                settings::AUTOSAVE_KEY.into(),
                settings::Val::Text("off".into()),
            );
            cx.set_global(app);
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-clear-range.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: vec!["Medium".into(), "Title".into()],
                    rows: vec![
                        vec!["Film".into(), "one".into()],
                        vec!["Video".into(), "two".into()],
                    ],
                    row_ids: vec![1, 2],
                    values: Default::default(),
                },
            });
        });
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);
        let state = panel.read_with(cx, |panel, _| panel.state.clone());

        cx.update(|_, cx| {
            state.update(cx, |state, _| {
                state.delegate_mut().selection =
                    Some(crate::delegate::Selection::Cell { row: 1, col: 0 });
                state.delegate_mut().range = Some(((1, 0), (1, 1)));
            });
        });

        let row1 = |cx: &mut gpui::VisualTestContext| {
            state.read_with(cx, |state, _| {
                (0..2)
                    .map(|col| state.delegate().cell(1, col).cloned().unwrap_or_default())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(row1(cx), vec!["Video", "two"]);

        panel.update(cx, |panel, cx| panel.clear_range(cx));
        assert_eq!(row1(cx), vec!["", ""], "both cells blanked");

        cx.update(|_, cx| crate::history_step(false, cx));
        assert_eq!(row1(cx), vec!["Video", "two"], "one undo puts both back");

        cx.update(|_, cx| crate::history_step(true, cx));
        assert_eq!(row1(cx), vec!["", ""], "and redo blanks them again");
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
}
