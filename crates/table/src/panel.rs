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

mod files;
pub(crate) mod watch;

const COLUMN_LAYOUT_KEY: &str = "table_columns";

fn map_spreadsheet_headers(
    source: &[String],
    destination: &[String],
) -> Result<(Vec<Option<usize>>, Vec<String>), String> {
    let mut used = std::collections::HashSet::new();
    let mut skipped = Vec::new();
    let mut mapping = Vec::with_capacity(source.len());
    for header in source {
        let target = destination
            .iter()
            .position(|name| name.trim().eq_ignore_ascii_case(header.trim()));
        if let Some(target) = target {
            if !used.insert(target) {
                return Err(format!(
                    "More than one source column matches '{}'.",
                    destination[target]
                ));
            }
        } else {
            skipped.push(if header.trim().is_empty() {
                "(blank header)".into()
            } else {
                header.clone()
            });
        }
        mapping.push(target);
    }
    if used.is_empty() {
        return Err("No source headers match columns in this project.".into());
    }
    Ok((mapping, skipped))
}

pub(crate) const FROZEN_COLUMNS_KEY: &str = "table_frozen_columns";

/// A grid row's height. Scaled with the rem, which is what the UI scale moves, so a row still
/// fits its text at 150%. Padding stays the library's: the floating editor is laid out against it.
/// Each line past the first adds one wrapped-cell line, the editor's line height.
pub(crate) fn row_height(compact: bool, lines: usize, rem: Pixels) -> Pixels {
    let base = if compact { 28. } else { 32. };
    px(base * f32::from(rem) / 16.)
        + rem * crate::editor::LINE_HEIGHT.0 * lines.saturating_sub(1) as f32
}

/// The whole number of lines closest to a dragged row height, within what Settings offers.
pub(crate) fn lines_at(compact: bool, height: Pixels, rem: Pixels) -> usize {
    let extra = (height - row_height(compact, 1, rem)) / (rem * crate::editor::LINE_HEIGHT.0);
    (extra.round().max(0.) as usize + 1).min(crate::MAX_ROW_LINES)
}

/// Push the settings the delegate caches into it. Called wherever either store changes, since the
/// delegate reads no settings itself — it has no `App` in the paths that need them.
fn apply_settings(delegate: &mut QrateTableDelegate, cx: &App) {
    if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
        delegate.set_declared(
            project
                .data
                .columns
                .iter()
                .map(|column| {
                    (
                        SharedString::from(column.name.clone()),
                        (
                            settings::columns::ColumnType::from_declared(&column.data_type),
                            SharedString::from(column.notes.trim().to_string()),
                        ),
                    )
                })
                .collect(),
        );
        delegate.default_level =
            settings::description::DescriptionConfig::from_values(&project.data.values)
                .file_level_key;
    }
    delegate.undo_cap = crate::undo_steps(cx);
    delegate.row_lines = crate::row_lines(cx);
    let column_settings = settings::columns::load(cx);
    delegate.text_modes = column_settings
        .iter()
        .filter(|(_, s)| s.text_mode != settings::columns::TextMode::Overflow)
        .map(|(key, s)| (SharedString::from(key.clone()), s.text_mode))
        .collect();
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
        DeleteSubtree,
        ImportFiles,
        ImportSpreadsheet,
        RelinkMissingFiles
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
    /// The search waiting for typing to pause. See [`Self::schedule_search`].
    _search_task: Option<Task<()>>,
    /// Linked documents being read for a search that includes them. `Some` while it runs.
    reading_documents: Option<Task<()>>,
    /// The pending visual query. Replacing it cancels the one before, which debounces typing.
    visual_query: Option<Task<()>>,
    /// The file a visual search with an empty query ranks against, from "Find similar items".
    similar: Option<std::path::PathBuf>,
    _visual_sub: Subscription,
    /// The last visual ranking as `(score, source_row, data_col)`, best first, so moving the
    /// breadth slider re-cuts it without asking the model again.
    ranking: Vec<(f32, usize)>,
    /// How far below the best visual hit still counts as a match.
    breadth: Entity<SliderState>,
    _breadth_sub: Subscription,
    replace_input: Entity<InputState>,
    replace_open: bool,
    _replace_sub: Subscription,
    /// Repaints the files-folder banner, which `ViewsPanel` draws from this view.
    _files_sub: Subscription,
    /// Pending debounced autosave (the "timed" mode). Replacing it drops the prior task, which
    /// cancels its timer — that drop *is* the debounce, coalescing a burst of edits into one write.
    _autosave_task: Option<Task<()>>,
    /// Pending debounced revalidation, on the same drop-cancels-the-timer trick as the autosave.
    _revalidate_task: Option<Task<()>>,
    /// The watcher on the files folder, while the project has one.
    watch: Option<watch::FolderWatch>,
    /// The row height a drag on a row-number edge is previewing, until it is released.
    row_drag: Option<Pixels>,
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
            log::info!(
                "opened {} with {} rows, {} arranged components, {} profile",
                project.file.display(),
                project.data.rows.len(),
                structure.len(),
                description.profile.key()
            );
            Self::apply_saved_layout(&mut delegate, &project.file);
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
        // A newly opened project reads its files folder afresh, off the UI thread.
        photos::forget(cx);
        cx.set_global(crate::file_links::FilesBase::default());
        cx.set_global(watch::NewFiles::default());
        state.update(cx, |state, cx| photos::refresh(state, None, cx));

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
                    this.sync_watch(cx);
                    cx.notify();
                    return;
                }
                log::info!("loading project {}", file.display());

                let (headers, row_ids, rows) = (
                    project.data.headers.clone(),
                    project.data.row_ids.clone(),
                    project.data.rows.clone(),
                );
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
                log::info!(
                    "loaded {} rows, {} arranged components, {} profile",
                    rows.len(),
                    structure.len(),
                    description.profile.key()
                );
                photos::forget(cx);
                cx.set_global(crate::file_links::FilesBase::default());
                this.state.update(cx, |state, cx| {
                    state.delegate_mut().set_data(&headers, &row_ids, &rows);
                    state
                        .delegate_mut()
                        .set_structure(&structure, &description.file_level_key);
                    state.delegate_mut().restore_expanded(&expanded);
                    Self::apply_saved_layout(state.delegate_mut(), &file);
                    apply_settings(state.delegate_mut(), cx);
                    photos::refresh(state, None, cx);
                    state.refresh(cx);
                    cx.emit(TableChanged);
                    cx.notify();
                });
                this.sync_watch(cx);
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
                    InputEvent::Change => this.schedule_search(cx),
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

        let _files_sub =
            cx.observe_global::<crate::file_links::FilesBase>(|_: &mut Self, cx| cx.notify());

        let _replace_sub = cx.subscribe(&replace_input, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.replace(false, cx);
            }
        });

        let mut panel = Self {
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
            _search_task: None,
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
            _files_sub,
            _autosave_task: None,
            _revalidate_task: None,
            watch: None,
            row_drag: None,
        };
        panel.sync_watch(cx);
        cx.set_global(crate::TablePanelHandle(cx.entity().downgrade()));
        panel
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

    /// Import dropped or picked files and folders as rows, once any from outside the files folder
    /// have been copied in or accepted where they are.
    pub fn import_external_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.place_outside_files(paths, window, cx, |this, paths, window, cx| {
            this.import_paths(paths, false, window, cx)
        });
    }

    /// Plan `paths` as rows and ask before adding them. `new_files` are the watch's new files,
    /// which the prompt also offers to ignore.
    fn import_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        new_files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offered = new_files.then(|| paths.clone());
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
        let policy = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|project| {
                project
                    .data
                    .values
                    .get(settings::project::IMPORT_DUPLICATE_POLICY_KEY)
            })
            .map(|value| file_ingest::duplicates::DuplicatePolicy::parse(&value.text()))
            .unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let plan = match task.await {
                Ok(plan) => plan,
                Err(error) => {
                    log::warn!(
                        "Nothing was imported: the dropped paths could not be read ({error:?})"
                    );
                    return;
                }
            };
            for warning in &plan.warnings {
                log::warn!("Left a dropped path out of the import: {warning:?}");
            }
            this.update_in(cx, |this, window, cx| {
                let project = cx.global::<settings::project::CurrentProject>();
                let files_root = project
                    .data
                    .values
                    .get(settings::project::FILES_FOLDER_KEY)
                    .map(|folder| std::path::PathBuf::from(folder.text().as_ref()));
                let (title_name, file_name) = project.data.columns.iter().fold(
                    (None, None),
                    |(title, file), column| {
                        match settings::columns::ColumnType::from_declared(&column.data_type) {
                            settings::columns::ColumnType::Title => {
                                (Some(column.name.clone()), file)
                            }
                            settings::columns::ColumnType::Filename => {
                                (title, Some(column.name.clone()))
                            }
                            _ => (title, file),
                        }
                    },
                );
                let (title_col, file_col) = {
                    let delegate = this.state.read(cx).delegate();
                    (
                        title_name.as_deref().and_then(|name| delegate.data_col(name)),
                        file_name.as_deref().and_then(|name| delegate.data_col(name)),
                    )
                };
                let existing = this
                    .state
                    .read(cx)
                    .delegate()
                    .existing_components(file_col, files_root.as_deref());
                let resolved = file_ingest::duplicates::resolve(
                    plan,
                    &existing,
                    settings::filenames::keys,
                    policy,
                );

                let files = resolved
                    .plan
                    .components
                    .iter()
                    .filter(|component| component.kind == file_ingest::EntryKind::File)
                    .count();
                let folders = resolved.plan.components.len() - files;
                let duplicates = resolved.duplicates();
                let detail = format!(
                    "Add {} component{} ({} file{}, {} folder{})?{}{}{}",
                    resolved.plan.components.len(),
                    if resolved.plan.components.len() == 1 { "" } else { "s" },
                    files,
                    if files == 1 { "" } else { "s" },
                    folders,
                    if folders == 1 { "" } else { "s" },
                    if duplicates == 0 {
                        String::new()
                    } else {
                        format!(" {duplicates} are already in this project.")
                    },
                    if resolved.ambiguous() == 0 {
                        String::new()
                    } else {
                        format!(
                            " {} match several rows and will be added as new rows.",
                            resolved.ambiguous()
                        )
                    },
                    if resolved.plan.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " {} path warning(s) will be skipped.",
                            resolved.plan.warnings.len()
                        )
                    }
                );
                // Only a drop that actually repeats material asks what to do about it, with the
                // project's saved policy as the default button.
                use file_ingest::duplicates::DuplicatePolicy;
                let mut order = [
                    DuplicatePolicy::Skip,
                    DuplicatePolicy::Update,
                    DuplicatePolicy::AddAsNew,
                ];
                order.sort_by_key(|option| *option != policy);
                let choices: Vec<&str> = match duplicates {
                    0 => vec!["Import"],
                    _ => order
                        .iter()
                        .map(|option| match option {
                            DuplicatePolicy::Skip => "Skip duplicates",
                            DuplicatePolicy::Update => "Update existing",
                            DuplicatePolicy::AddAsNew => "Add all as new",
                        })
                        .collect(),
                };
                let ignore_at = offered.as_ref().map(|_| choices.len());
                let choices: Vec<&str> = choices
                    .into_iter()
                    .chain(offered.as_ref().map(|_| "Ignore"))
                    .chain(["Cancel"])
                    .collect();
                let answer = window.prompt(
                    PromptLevel::Info,
                    match offered {
                        Some(_) => "Import new files",
                        None => "Import dropped files",
                    },
                    Some(&detail),
                    &choices,
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let chosen = answer.await.unwrap_or(usize::MAX);
                    if let Some(offered) = &offered
                        && ignore_at == Some(chosen)
                    {
                        cx.update(|_, cx| watch::NewFiles::settle(offered, true, cx))
                            .ok();
                        return;
                    }
                    let chosen = match (duplicates, chosen) {
                        (0, 0) => policy,
                        (1.., ix) if ix < order.len() => order[ix],
                        _ => return,
                    };
                    this.update(cx, |this, cx| {
                        if let Some(offered) = &offered {
                            watch::NewFiles::settle(offered, false, cx);
                        }
                        if duplicates > 0 && chosen != policy {
                            settings::project::CurrentProject::set_text(
                                settings::project::IMPORT_DUPLICATE_POLICY_KEY,
                                chosen.key().into(),
                                cx,
                            );
                        }
                        // Re-resolving is what makes the buttons mean what they say.
                        let resolved = match chosen == policy {
                            true => resolved,
                            false => file_ingest::duplicates::resolve(
                                resolved.plan,
                                &existing,
                                settings::filenames::keys,
                                chosen,
                            ),
                        };
                        this.state.update(cx, |state, cx| {
                            let added = state.delegate_mut().append_components(
                                &resolved,
                                title_col,
                                file_col,
                                None,
                                files_root.as_deref(),
                            );
                            log::info!(
                                "imported {added} dropped component(s) as {}, reusing {} already in the project",
                                chosen.key(),
                                resolved.duplicates()
                            );
                            state.refresh(cx);
                            cx.emit(TableChanged);
                            cx.notify();
                        });
                        let row_ids = this.state.read(cx).delegate().row_ids().to_vec();
                        diagnostics::Diagnostics::align_note_rows(
                            diagnostics::DATASET_MAIN,
                            &row_ids,
                            settings::history::Origin::Structure,
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

    pub fn choose_import_paths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Choose files or folders to import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && !paths.is_empty()
            {
                this.update_in(cx, |this, window, cx| {
                    this.import_external_paths(paths, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn choose_import_spreadsheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an Excel, CSV, TSV, or ODS file to append".into()),
        });
        let project_file = cx
            .global::<settings::project::CurrentProject>()
            .file
            .clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let display = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let task = cx.background_executor().spawn(async move {
                data_exchange::spreadsheet::read_grid(&path.to_string_lossy())
            });
            let grid = task.await;
            this.update_in(cx, |this, window, cx| {
                if cx.global::<settings::project::CurrentProject>().file != project_file {
                    return;
                }
                let (headers, rows) = match grid {
                    Ok(grid) => grid,
                    Err(error) => {
                        let detail = error.message();
                        drop(window.prompt(
                            PromptLevel::Warning,
                            "Could not import spreadsheet",
                            Some(&detail),
                            &["OK"],
                            cx,
                        ));
                        return;
                    }
                };
                if headers.iter().all(|header| header.trim().is_empty()) || rows.is_empty() {
                    drop(window.prompt(
                        PromptLevel::Warning,
                        "Nothing to import",
                        Some("The file needs a header row and at least one data row."),
                        &["OK"],
                        cx,
                    ));
                    return;
                }
                let destination = {
                    let state = this.state.read(cx);
                    let delegate = state.delegate();
                    (0..delegate.column_count())
                        .map(|col| delegate.column_name(col).to_string())
                        .collect::<Vec<_>>()
                };
                let (mapping, skipped) = match map_spreadsheet_headers(&headers, &destination) {
                    Ok(mapping) => mapping,
                    Err(message) => {
                        drop(window.prompt(
                            PromptLevel::Warning,
                            "Could not import spreadsheet",
                            Some(&message),
                            &["OK"],
                            cx,
                        ));
                        return;
                    }
                };
                let matched = mapping.iter().filter(|column| column.is_some()).count();
                let detail = format!(
                    "Append {} row{} from {display}? {matched} column{} match.{}",
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" },
                    if matched == 1 { "" } else { "s" },
                    if skipped.is_empty() {
                        String::new()
                    } else {
                        format!(" Skipping unmatched columns: {}.", skipped.join(", "))
                    }
                );
                let answer = window.prompt(
                    PromptLevel::Info,
                    "Import spreadsheet",
                    Some(&detail),
                    &["Import", "Cancel"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await != Ok(0) {
                        return;
                    }
                    this.update(cx, |this, cx| {
                        if cx.global::<settings::project::CurrentProject>().file != project_file {
                            return;
                        }
                        let current_headers = {
                            let state = this.state.read(cx);
                            let delegate = state.delegate();
                            (0..delegate.column_count())
                                .map(|col| delegate.column_name(col).to_string())
                                .collect::<Vec<_>>()
                        };
                        if current_headers != destination {
                            return;
                        }
                        let values = rows
                            .into_iter()
                            .map(|source| {
                                let mut cells = vec![SharedString::default(); destination.len()];
                                for (from, to) in mapping.iter().enumerate() {
                                    if let Some(to) = to {
                                        cells[*to] =
                                            source.get(from).cloned().unwrap_or_default().into();
                                    }
                                }
                                cells
                            })
                            .collect();
                        this.state.update(cx, |state, cx| {
                            let at = state.delegate().row_count();
                            let added = state.delegate_mut().append_spreadsheet_rows(values);
                            let rows: Vec<usize> = (at..at + added).collect();
                            photos::refresh(state, Some(&rows), cx);
                            state.refresh(cx);
                            cx.emit(TableChanged);
                            cx.notify();
                        });
                        let row_ids = this.state.read(cx).delegate().row_ids().to_vec();
                        diagnostics::Diagnostics::align_note_rows(
                            diagnostics::DATASET_MAIN,
                            &row_ids,
                            Origin::Import,
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
        crate::stamp_pending_author(cx);
        match settings::effective_text(settings::AUTOSAVE_KEY, cx).as_ref() {
            "off" => {}
            "immediate" => crate::autosave(cx),
            _ => {
                self._autosave_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(AUTOSAVE_DEBOUNCE).await;
                    this.update(cx, |_this, cx| crate::autosave(cx)).ok();
                }));
            }
        }
    }

    /// Re-run the search once typing in the find bar pauses. Replacing the task cancels the timer
    /// of the keystroke before, so a burst of typing scans the grid once.
    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self._search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |this, cx| this.refresh_search(cx)).ok();
        }));
    }

    /// Recompute the find matches from the current query and jump to the first, if any.
    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        self._search_task = None;
        if self.search_opts.visual {
            self.search_visual(cx);
            return;
        }
        self.ranking.clear();
        let needle = self.search_input.read(cx).value().to_string();
        let query = compile_search(&needle, self.search_opts);
        self.search_error = !needle.trim().is_empty() && query.is_none();
        let opts = self.search_opts;
        // A search that reaches into linked files narrows the view to its hits.
        let hits = query
            .as_ref()
            .filter(|_| opts.files)
            .map(|re| self.state.read(cx).delegate().hit_rows(re, opts));
        self.set_search_rows(hits, cx);
        self.search_matches = query
            .as_ref()
            .map(|re| self.state.read(cx).delegate().search_matches(re, opts))
            .unwrap_or_default();
        self.search_ix = 0;
        self.select_current_match(cx);
        self.read_documents(cx);
        cx.notify();
    }

    /// Rank rows by how well their linked file matches the query, or looks like the file "Find
    /// similar items" was raised on when the query is empty. Typing waits for a pause first, and
    /// the scoring runs off the UI thread.
    fn search_visual(&mut self, cx: &mut Context<Self>) {
        let files = self.state.read(cx).delegate().linked_files();
        visual::index(files, cx);
        self.search_error = false;
        let needle = self.search_input.read(cx).value().trim().to_string();
        let (query, pause) = match (needle.is_empty(), self.similar.clone()) {
            (false, _) => (visual::Query::Text(needle), VISUAL_DEBOUNCE),
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
            let Ok(candidates) =
                this.update(cx, |this, cx| this.state.read(cx).delegate().linked_rows())
            else {
                return;
            };
            // No scorer means the model is not loaded yet: show every row until it is.
            let ranking = match score {
                Some(score) => {
                    cx.background_executor()
                        .spawn(async move { score(&candidates) })
                        .await
                }
                None => Vec::new(),
            };
            this.update(cx, |this, cx| {
                this.ranking = ranking;
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
        let hits: Vec<usize> = self.ranking[..keep].iter().map(|hit| hit.1).collect();
        self.set_search_rows((!hits.is_empty()).then(|| hits.clone()), cx);
        let delegate = self.state.read(cx).delegate();
        let kept: std::collections::HashSet<usize> = hits.into_iter().collect();
        self.search_matches = delegate
            .visible()
            .iter()
            .enumerate()
            .filter(|(_, source)| kept.contains(source))
            .map(|(view, &source)| (view, delegate.file_column(source)))
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
        let unread = self
            .state
            .update(cx, |state, _| state.delegate_mut().unread_documents());
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
            let opts = self.search_opts;
            self.search_matches = compile_search(&needle, opts)
                .map(|re| self.state.read(cx).delegate().search_matches(&re, opts))
                .unwrap_or_default();
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
        let Some(re) = compile_search(&needle, self.search_opts) else {
            return;
        };
        let replacement = self.replace_input.read(cx).value().to_string();
        let only = match all {
            true => None,
            false => match self.search_matches.get(self.search_ix) {
                Some(&hit) => Some(hit),
                None => return,
            },
        };
        let cells =
            self.state
                .read(cx)
                .delegate()
                .replace_edits(&re, &replacement, self.search_opts, only);
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
            let files = crate::file_rows(state.delegate(), &cells);
            state.delegate_mut().apply_edit(cells, origin);
            if let Some(rows) = files {
                photos::refresh(state, Some(&rows), cx);
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
                                .label(visual::download_label(cx))
                                .on_click(|_, _, cx| {
                                    components::install(components::ComponentId::Clip, cx).detach()
                                }),
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

/// App-level handlers for the grid's menu commands, so the menu bar reaches them wherever focus
/// sits rather than only while the grid holds it. Undo, Redo and Deselect are the app's own.
pub fn register_global_actions(cx: &mut App) {
    fn on_panel(cx: &mut App, run: impl FnOnce(&mut TablePanel, &mut Context<TablePanel>)) {
        if let Some(panel) = cx
            .try_global::<crate::TablePanelHandle>()
            .and_then(|handle| handle.0.upgrade())
        {
            panel.update(cx, run);
        }
    }
    // A global handler runs while the dispatching window is taken out; wait for it to come back.
    fn in_window(
        cx: &mut App,
        run: impl FnOnce(&mut TablePanel, &mut Window, &mut Context<TablePanel>) + 'static,
    ) {
        cx.defer(move |cx| {
            let panel = cx
                .try_global::<crate::TablePanelHandle>()
                .and_then(|handle| handle.0.upgrade());
            if let (Some(panel), Some(window)) = (panel, cx.active_window()) {
                window
                    .update(cx, |_, window, cx| {
                        panel.update(cx, |panel, cx| run(panel, window, cx))
                    })
                    .ok();
            }
        });
    }
    fn arrange(this: &mut TablePanel, op: fn(usize) -> crate::Arrangement, cx: &mut App) {
        if let Some((rows, _)) = this.structural_target(cx) {
            crate::arrange(op(rows[0]), cx);
        }
    }

    cx.on_action(|_: &InsertRowAbove, cx| {
        on_panel(cx, |this, cx| {
            this.structural(|rows, _| crate::Structural::InsertRow { at: rows[0] }, cx)
        })
    });
    cx.on_action(|_: &InsertRowBelow, cx| {
        on_panel(cx, |this, cx| {
            this.structural(
                |rows, _| crate::Structural::InsertRow {
                    at: rows[rows.len() - 1] + 1,
                },
                cx,
            )
        })
    });
    cx.on_action(|_: &DuplicateRow, cx| {
        on_panel(cx, |this, cx| {
            this.structural(
                |rows, _| crate::Structural::DuplicateRow { row: rows[0] },
                cx,
            )
        })
    });
    cx.on_action(|_: &DeleteRow, cx| {
        on_panel(cx, |this, cx| {
            this.structural(|rows, _| crate::Structural::DeleteRows(rows.to_vec()), cx)
        })
    });
    cx.on_action(|_: &InsertColumnLeft, cx| {
        on_panel(cx, |this, cx| {
            this.structural(|_, col| crate::Structural::InsertColumn { at: col }, cx)
        })
    });
    cx.on_action(|_: &InsertColumnRight, cx| {
        on_panel(cx, |this, cx| {
            this.structural(|_, col| crate::Structural::InsertColumn { at: col + 1 }, cx)
        })
    });
    cx.on_action(|_: &DeleteColumn, cx| {
        on_panel(cx, |this, cx| {
            this.structural(|_, col| crate::Structural::DeleteColumn { col }, cx)
        })
    });
    cx.on_action(|_: &IndentRow, cx| {
        on_panel(cx, |this, cx| arrange(this, crate::Arrangement::Indent, cx))
    });
    cx.on_action(|_: &OutdentRow, cx| {
        on_panel(cx, |this, cx| {
            arrange(this, crate::Arrangement::Outdent, cx)
        })
    });
    cx.on_action(|_: &DeleteSubtree, cx| {
        on_panel(cx, |this, cx| {
            arrange(this, crate::Arrangement::DeleteSubtree, cx)
        })
    });
    cx.on_action(|_: &UnfreezeColumns, cx| {
        on_panel(cx, |this, cx| {
            crate::set_frozen_columns(&this.state.clone(), 0, cx)
        })
    });
    cx.on_action(|_: &ExpandAll, cx| {
        on_panel(cx, |this, cx| {
            let expanded = this.state.update(cx, |state, cx| {
                state.delegate_mut().expand_all();
                let expanded = state.delegate().expanded_rows();
                state.refresh(cx);
                cx.emit(TableChanged);
                expanded
            });
            crate::persist_expanded(&expanded, cx);
        })
    });
    cx.on_action(|_: &CollapseAll, cx| {
        on_panel(cx, |this, cx| {
            this.state.update(cx, |state, cx| {
                state.delegate_mut().collapse_all();
                state.refresh(cx);
                cx.emit(TableChanged);
            });
            crate::persist_expanded(&[], cx);
        })
    });
    cx.on_action(|_: &RenameColumn, cx| {
        in_window(cx, |this, window, cx| this.rename_selected(window, cx))
    });
    cx.on_action(|_: &InsertNote, cx| {
        in_window(cx, |this, window, cx| {
            note::open_on_selection(&this.state.clone(), window, cx)
        })
    });
    cx.on_action(|_: &ImportFiles, cx| {
        in_window(cx, |this, window, cx| this.choose_import_paths(window, cx))
    });
    cx.on_action(|_: &ImportSpreadsheet, cx| {
        in_window(cx, |this, window, cx| {
            this.choose_import_spreadsheet(window, cx)
        })
    });
    cx.on_action(|_: &RelinkMissingFiles, cx| {
        in_window(cx, |this, window, cx| this.choose_files_root(window, cx))
    });
    cx.on_action(|_: &Search, cx| in_window(cx, |this, window, cx| this.toggle_search(window, cx)));
    cx.on_action(|_: &Replace, cx| in_window(cx, |this, window, cx| this.open_replace(window, cx)));
}

impl Focusable for TablePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TablePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stripe = settings::effective_bool(crate::TABLE_STRIPES_KEY, cx);
        let compact = settings::effective_text(crate::ROW_DENSITY_KEY, cx).as_ref() == "compact";
        let rem = window.rem_size();
        if !cx.has_active_drag() {
            self.row_drag = None;
        }
        let height = self
            .row_drag
            .unwrap_or_else(|| row_height(compact, crate::row_lines(cx), rem));

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
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<row_index::RowResize>, _, cx| {
                    let from = cx
                        .try_global::<row_index::RowResizeFrom>()
                        .map_or(event.event.position.y, |from| from.0);
                    let lines = crate::row_lines(cx);
                    this.row_drag = Some(
                        (row_height(compact, lines, rem) + event.event.position.y - from).clamp(
                            row_height(compact, 1, rem),
                            row_height(compact, crate::MAX_ROW_LINES, rem),
                        ),
                    );
                    cx.notify();
                },
            ))
            .on_drop(cx.listener(move |this, _: &row_index::RowResize, _, cx| {
                if let Some(height) = this.row_drag.take() {
                    crate::set_row_lines(lines_at(compact, height, rem), cx);
                }
                cx.notify();
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
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy_range(false, cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.copy_range(true, cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste_range(cx)))
            .on_action(cx.listener(|this, _: &Clear, _, cx| this.clear_range(cx)))
            .p_2()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when(compact, |table| table.text_sm())
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
                    .child(
                        DataTable::new(&self.state)
                            .bordered(false)
                            .stripe(stripe)
                            .with_size(gpui_component::Size::Size(height)),
                    )
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

const AUTOSAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(800);

/// How long the find bar waits for typing to pause before it scans the grid.
const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

/// How long typing pauses before a visual query asks the model.
const VISUAL_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

const SUGGEST_W: f32 = 220.;
const SUGGEST_H: f32 = 180.;

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use diagnostics::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source};
    use gpui::{SharedString, TestAppContext};
    use settings::history::Origin;

    #[test]
    fn row_height_follows_density_lines_and_the_ui_scale() {
        use gpui::px;
        assert_eq!(super::row_height(false, 1, px(16.)), px(32.));
        assert_eq!(super::row_height(true, 1, px(16.)), px(28.));
        assert_eq!(super::row_height(false, 1, px(24.)), px(48.));
        assert_eq!(super::row_height(false, 2, px(16.)), px(52.));
        assert_eq!(super::row_height(true, 4, px(16.)), px(88.));
        assert_eq!(super::row_height(false, 3, px(24.)), px(108.));
    }

    #[test]
    fn a_dragged_height_snaps_to_the_nearest_line_within_range() {
        use gpui::px;
        for compact in [false, true] {
            for rem in [px(16.), px(24.)] {
                for lines in 1..=crate::MAX_ROW_LINES {
                    let exact = super::row_height(compact, lines, rem);
                    assert_eq!(super::lines_at(compact, exact, rem), lines);
                    assert_eq!(super::lines_at(compact, exact + px(4.), rem), lines);
                }
            }
        }
        assert_eq!(super::lines_at(false, px(0.), px(16.)), 1);
        assert_eq!(
            super::lines_at(false, px(500.), px(16.)),
            crate::MAX_ROW_LINES
        );
    }

    #[test]
    fn spreadsheet_headers_map_by_name_and_reject_ambiguous_sources() {
        let destination = vec!["Title".into(), "Digital ID".into()];
        let source = vec![" digital id ".into(), "Extra".into(), "TITLE".into()];
        let (mapping, skipped) = super::map_spreadsheet_headers(&source, &destination).unwrap();
        assert_eq!(mapping, vec![Some(1), None, Some(0)]);
        assert_eq!(skipped, vec!["Extra"]);
        assert!(
            super::map_spreadsheet_headers(&["Title".into(), "title".into()], &destination)
                .is_err()
        );
    }

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
    fn a_wrap_column_draws_at_every_row_height(cx: &mut TestAppContext) {
        use gpui::BorrowAppContext as _;
        use settings::columns::TextMode;
        project_with_notes(cx);
        cx.update(|cx| {
            cx.update_global::<settings::project::CurrentProject, _>(|project, _| {
                project.data.rows = vec![
                    vec![
                        "a value long enough to wrap onto more lines than any row holds ".repeat(8),
                    ],
                    vec!["short".into()],
                ];
                project.data.values.insert(
                    settings::columns::COLUMN_SETTINGS_KEY.into(),
                    settings::Val::Text(r#"{"Title":{"text_mode":"wrap"}}"#.into()),
                );
            });
        });
        let (panel, cx) = cx.add_window_view(super::TablePanel::new);
        for lines in 1..=crate::MAX_ROW_LINES {
            cx.update(|_, cx| {
                cx.update_global::<settings::AppSettings, _>(|app, _| {
                    app.values.insert(
                        crate::ROW_LINES_KEY.into(),
                        settings::Val::Text(lines.to_string().into()),
                    );
                });
            });
            cx.run_until_parked();
            cx.draw(
                gpui::point(gpui::px(0.), gpui::px(0.)),
                gpui::size(gpui::px(400.), gpui::px(300.)),
                |_, _| gpui::IntoElement::into_any_element(panel.clone()),
            );
            panel.update(cx, |panel, cx| {
                let delegate = panel.state.read(cx).delegate();
                assert_eq!(delegate.row_lines, lines);
                assert_eq!(delegate.text_mode(0), TextMode::Wrap);
                assert!(cx.has_global::<crate::TableViewportBounds>());
            });
        }
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
