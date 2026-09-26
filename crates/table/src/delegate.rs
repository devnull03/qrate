use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Pixels, SharedString,
    Task, Window, div, px,
};
use gpui_component::{
    input::TextareaState,
    table::{Column, TableDelegate, TableState},
};
use serde::{Deserialize, Serialize};

use diagnostics::{DATASET_MAIN, Location};
use settings::columns::ColumnType;
use settings::history::{Change, Entry, EntryId, Origin};
use settings::project::RowId;

use crate::history::{Cells, Col, History, Row, Step, Structure};
use crate::{
    cell,
    editing::EditState,
    filter,
    hierarchy::{Hierarchy, Placement},
    row_index,
};

/// Emitted whenever the table's selection, a cell's text, or the column layout changes, so
/// cross-crate listeners know to re-render.
pub struct TableChanged;

impl EventEmitter<TableChanged> for TableState<QrateTableDelegate> {}

/// The table's current selection. Columns are data-relative (the pinned `#` column excluded) and
/// 0-based; rows are source-data indices, so a selection survives a filter change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Cell { row: usize, col: usize },
    Row(usize),
    Column(usize),
}

/// Saved column layout — display order + widths — persisted into the project's `.qrate` file.
/// A layout whose key set doesn't match the current data is ignored.
#[derive(Serialize, Deserialize)]
pub struct ColumnLayout {
    pub keys: Vec<String>,
    pub widths: Vec<f32>,
}

pub type DatasetSnapshot = (Vec<String>, Vec<settings::project::RowId>, Vec<Vec<String>>);

/// Everything a save writes, taken on the UI thread and written wherever the caller likes.
pub(crate) struct SaveSnapshot {
    pub headers: Vec<SharedString>,
    pub row_ids: Vec<RowId>,
    pub rows: Vec<Vec<SharedString>>,
    pub structure: Structure,
    pub history: Vec<Entry>,
    /// The log position `history[0]` holds, counted from when the dataset was loaded.
    pub first: u64,
    /// [`QrateTableDelegate::edits`] when this was taken.
    pub edits: u64,
    pub ledger: Arc<SaveLedger>,
}

/// One dataset's saves, which may overlap: a background autosave still writing when Ctrl+S comes.
/// `write` takes them in turn; the counters say what has reached the file, so a later save skips
/// log entries already there and a stale one writes nothing over a newer one.
#[derive(Default)]
pub(crate) struct SaveLedger {
    pub write: Mutex<()>,
    /// Log entries on disk, as a position like [`SaveSnapshot::first`].
    pub written: AtomicU64,
    /// [`SaveSnapshot::edits`] of the newest snapshot on disk.
    pub edits: AtomicU64,
}

/// Data + column model for the center table. In `gpui_component` the delegate *is* the model:
/// the virtualized `DataTable` calls back into it for counts and per-cell rendering.
pub struct QrateTableDelegate {
    columns: Vec<Column>,
    rows: Vec<Vec<SharedString>>,
    /// Stable SQLite identities parallel to `rows`. Source indices remain the UI coordinate.
    row_ids: Vec<settings::project::RowId>,
    next_row_id: settings::project::RowId,
    /// Last selection reported by the table's native `TableEvent`s, written only by
    /// `TablePanel`'s event bridge (the library's `selected_cell()` goes stale in row mode).
    pub(crate) selection: Option<Selection>,
    /// Shift-click range as `(anchor, head)`, each a `(view_row, data_col)` pair. View coordinates
    /// because every rendered cell tests against this, and a source row would cost a linear lookup
    /// per cell. `None` means the selection is just the one selected cell. Dropped wherever a
    /// filter or a column move invalidates a view index.
    pub(crate) range: Option<((usize, usize), (usize, usize))>,
    /// Rows picked out by ⌘-click or a shift-click down the `#` column, as *source* indices — so a
    /// multi-selection survives a filter change exactly as the single cursor does. Empty is the
    /// common case: one selected row lives in `selection` alone and never reaches this set.
    pub(crate) selected_rows: BTreeSet<usize>,
    pub(crate) editing: EditState,
    /// Shared cell editor, reused across whichever cell is being edited. Multi-line so a long
    /// value wraps inside the editor box instead of running off its right edge; Enter still
    /// commits it, and Shift+Enter is what inserts a newline.
    pub(crate) editor: Entity<TextareaState>,
    /// Which cell/row/column the note editor is open on, if any.
    pub(crate) note_edit: Option<Location>,
    /// Shared note editor, the same one-per-table arrangement as [`Self::editor`].
    pub(crate) note_editor: Entity<TextareaState>,
    /// Each row's resolved image path, parallel to `rows`. `None` until `TablePanel` resolves it.
    image_paths: Vec<Option<PathBuf>>,
    /// The text inside linked documents, by path. Filled by `TablePanel` the first time a search
    /// includes linked files; a file with no text layer holds an empty string so it is not re-read.
    document_text: HashMap<PathBuf, String>,
    /// [`Self::unread_documents`], kept until the linked files or the texts read change.
    unread: Option<Vec<PathBuf>>,
    /// The pending re-resolution of `image_paths` against the files folder. See `photos::refresh`.
    pub(crate) images_task: Option<Task<()>>,
    /// Each declared column's type and description, by name. Pushed in by `panel::apply_settings`
    /// so a rendered cell never searches the project's column list.
    declared: HashMap<SharedString, (ColumnType, SharedString)>,
    /// View→source row mapping: `visible_rows[view] == source`. The library only ever sees this
    /// narrowed set, so filtering composes with the virtualized render for free.
    visible_rows: Vec<usize>,
    /// Source→view, the inverse of `visible_rows`, rebuilt with it.
    view_of: Vec<Option<usize>>,
    /// The rows the column filters let through, before a search narrows them. What searches scan,
    /// so typing more can widen the hits again.
    filtered_rows: Vec<usize>,
    /// A visual or linked-file search's hits as source rows, in the order the view shows them.
    search_rows: Option<Vec<usize>>,
    /// Hierarchy depth parallel to `visible_rows`.
    visible_depths: Vec<usize>,
    hierarchy: Hierarchy,
    pub(crate) default_level: String,
    /// Per-data-column set of *excluded* cell values, parallel to `columns`. Empty = no filter.
    filters: Vec<HashSet<SharedString>>,
    /// Whether each column offers a filter dropdown at all, parallel to `columns`. Off for every
    /// column until switched on per column — filtering is opt-in.
    filters_enabled: Vec<bool>,
    /// Splits one cell into several filterable values. Empty (the default) means a cell is one
    /// value, which is how filtering behaved before this existed.
    subdelimiter: SharedString,
    /// Bumped whenever anything a filter dropdown lists could have changed. The dropdown caches its
    /// value list and only recomputes it when this moves — [`Self::column_values`] is O(rows), and
    /// the alternative is paying it on every frame the header renders.
    values_generation: u64,
    /// Undo/redo stack for cell edits and shape changes alike, so a delete and the typing before
    /// it come back in the order they went in.
    history: History,
    /// How many steps `history` keeps; [`crate::undo_steps`], pushed in with the other settings.
    pub(crate) undo_cap: usize,
    /// Log entries for changes not yet saved. They reach the `.qrate` file with the data they
    /// describe, and are dropped with it when a project is closed without saving.
    unsaved: Vec<Entry>,
    stamped_history: usize,
    /// How many log entries have left `unsaved` for the file since the dataset was loaded.
    saved_history: u64,
    /// Counts every recorded change, so a save can tell whether the grid moved on while it wrote.
    edits: u64,
    ledger: Arc<SaveLedger>,
    /// How many leading data columns are frozen. A count in *display* order, not a set of keys:
    /// the library's fixed region is always the leading columns, and moving a column in or out of
    /// it is how a sheet re-freezes. Zero means only the pinned `#` column stays put.
    frozen: usize,
}

impl QrateTableDelegate {
    pub(crate) fn new(editor: Entity<TextareaState>, note_editor: Entity<TextareaState>) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            row_ids: Vec::new(),
            next_row_id: 1,
            selection: None,
            range: None,
            selected_rows: BTreeSet::new(),
            editing: EditState::Idle,
            editor,
            note_edit: None,
            note_editor,
            image_paths: Vec::new(),
            document_text: HashMap::new(),
            unread: None,
            images_task: None,
            declared: HashMap::new(),
            visible_rows: Vec::new(),
            view_of: Vec::new(),
            filtered_rows: Vec::new(),
            search_rows: None,
            visible_depths: Vec::new(),
            hierarchy: Hierarchy::default(),
            default_level: "item".into(),
            filters: Vec::new(),
            filters_enabled: Vec::new(),
            subdelimiter: SharedString::default(),
            values_generation: 0,
            history: History::default(),
            undo_cap: crate::DEFAULT_UNDO_STEPS,
            unsaved: Vec::new(),
            stamped_history: 0,
            saved_history: 0,
            edits: 0,
            ledger: Arc::default(),
            frozen: 0,
        }
    }

    /// Freeze the leading `count` data columns, clamped to what exists. Zero unfreezes.
    pub(crate) fn set_frozen(&mut self, count: usize) {
        self.frozen = count.min(self.columns.len());
    }

    pub(crate) fn frozen(&self) -> usize {
        self.frozen
    }

    /// Data columns only — the pinned `#` column is the library's, not the data's.
    pub(crate) fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// See [`Self::values_generation`].
    pub fn values_generation(&self) -> u64 {
        self.values_generation
    }

    /// Source rows, including ones a filter currently hides.
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// View→source conversion at the single library boundary. `None` for a view index that's
    /// stale for the current filtered set.
    pub fn source(&self, view: usize) -> Option<usize> {
        self.visible_rows.get(view).copied()
    }

    /// Every visible row, in view order, as source indices. What a view that isn't the library's
    /// `DataTable` iterates, so a column filter narrows every view the same way.
    pub fn visible(&self) -> &[usize] {
        &self.visible_rows
    }

    pub fn row_depth(&self, view: usize) -> usize {
        self.visible_depths.get(view).copied().unwrap_or_default()
    }

    pub(crate) fn row_child_count(&self, source: usize) -> usize {
        self.row_ids
            .get(source)
            .map_or(0, |row_id| self.hierarchy.child_count(*row_id))
    }

    pub(crate) fn row_has_children(&self, source: usize) -> bool {
        self.row_ids
            .get(source)
            .is_some_and(|row_id| self.hierarchy.has_children(*row_id))
    }

    pub(crate) fn row_expanded(&self, source: usize) -> bool {
        self.row_ids
            .get(source)
            .is_some_and(|row_id| self.hierarchy.is_expanded(*row_id))
    }

    pub(crate) fn toggle_expanded(&mut self, source: usize) {
        let Some(row_id) = self.row_ids.get(source).copied() else {
            return;
        };
        self.hierarchy
            .set_expanded(row_id, !self.hierarchy.is_expanded(row_id));
        self.recompute_visible();
    }

    pub(crate) fn expand_all(&mut self) {
        self.hierarchy.expand_all();
        self.recompute_visible();
    }

    pub(crate) fn collapse_all(&mut self) {
        self.hierarchy.collapse_all();
        self.recompute_visible();
    }

    pub(crate) fn expanded_rows(&self) -> Vec<settings::project::RowId> {
        self.hierarchy.expanded_rows()
    }

    pub(crate) fn restore_expanded(&mut self, row_ids: &[settings::project::RowId]) {
        self.hierarchy.restore_expanded(row_ids);
        self.recompute_visible();
    }

    /// Source→view, the inverse of [`source`](Self::source). `None` when a filter currently hides
    /// the row — a diagnostic can point at a row the user has narrowed away.
    pub fn view_row(&self, source: usize) -> Option<usize> {
        self.view_of.get(source).copied().flatten()
    }

    /// A data-column index from its header text — which is also the column's key, so this is the
    /// one lookup every other addressing scheme in the app resolves through.
    pub fn data_col(&self, header: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == header)
    }

    /// A diagnostic [`Location`] from the delegate's own coordinates — a source row, a data
    /// column, or both. The only place `DATASET_MAIN` and the positional-column→name lookup meet.
    pub(crate) fn location(&self, row: Option<usize>, data_col: Option<usize>) -> Location {
        Location {
            dataset: DATASET_MAIN.into(),
            row,
            row_id: row.and_then(|source| self.row_ids.get(source).copied()),
            column: data_col.map(|c| self.column_name(c)),
        }
    }

    /// Narrow a column to one cell's values. Switches the dropdown on too, so the narrowing is
    /// visible in the header and reversible without going back through the cell. A sub-delimited
    /// cell re-checks each of its parts — the whole string isn't in the checklist to re-check.
    pub(crate) fn keep_only_value(&mut self, data_col: usize, value: &SharedString) {
        let parts: Vec<SharedString> = cell_parts(value, &self.subdelimiter)
            .into_iter()
            .map(SharedString::from)
            .collect();
        self.set_column_filter_enabled(data_col, true);
        self.exclude_all_in_column(data_col);
        for part in parts {
            self.toggle_filter_value(data_col, &part);
        }
    }

    fn recompute_visible(&mut self) {
        self.filtered_rows = compute_visible_rows(&self.rows, &self.filters, &self.subdelimiter);
        if let Some(hits) = &self.search_rows {
            let kept: HashSet<usize> = self.filtered_rows.iter().copied().collect();
            let all_ids = self.row_ids.iter().copied().collect();
            let depths: HashMap<_, _> = self
                .hierarchy
                .projection(Some(&all_ids))
                .into_iter()
                .collect();
            self.visible_rows = hits
                .iter()
                .copied()
                .filter(|row| kept.contains(row))
                .collect();
            self.visible_depths = self
                .visible_rows
                .iter()
                .map(|source| {
                    self.row_ids
                        .get(*source)
                        .and_then(|row_id| depths.get(row_id))
                        .copied()
                        .unwrap_or_default()
                })
                .collect();
        } else {
            let matching_ids = (self.filtered_rows.len() != self.rows.len()).then(|| {
                self.filtered_rows
                    .iter()
                    .filter_map(|source| self.row_ids.get(*source).copied())
                    .collect::<HashSet<_>>()
            });
            let source_by_id: HashMap<_, _> = self
                .row_ids
                .iter()
                .enumerate()
                .map(|(source, row_id)| (*row_id, source))
                .collect();
            let projection = self.hierarchy.projection(matching_ids.as_ref());
            self.visible_rows.clear();
            self.visible_depths.clear();
            for (row_id, depth) in projection {
                if let Some(source) = source_by_id.get(&row_id) {
                    self.visible_rows.push(*source);
                    self.visible_depths.push(depth);
                }
            }
        }
        self.view_of = vec![None; self.rows.len()];
        for (view, &source) in self.visible_rows.iter().enumerate() {
            if let Some(slot) = self.view_of.get_mut(source) {
                *slot = Some(view);
            }
        }
        // `range` is in view coordinates, which this just redefined.
        self.range = None;
    }

    /// The distinct values in a data column, sorted. Drawn from *all* source rows, so an excluded
    /// value can be re-checked to bring its rows back, and split on the sub-delimiter — a column of
    /// `Film; Video` is a checklist of two subjects, not one entry per combination.
    pub(crate) fn column_values(&self, data_col: usize) -> Vec<SharedString> {
        self.rows
            .iter()
            .filter_map(|row| row.get(data_col))
            .flat_map(|value| cell_parts(value, &self.subdelimiter))
            .map(SharedString::from)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Toggle a value in/out of a column's excluded set, then renarrow. Drops any in-flight edit,
    /// whose cell may have just been filtered out from under the editor.
    pub(crate) fn toggle_filter_value(&mut self, data_col: usize, value: &SharedString) {
        if let Some(excluded) = self.filters.get_mut(data_col)
            && !excluded.remove(value)
        {
            excluded.insert(value.clone());
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    pub(crate) fn is_filter_excluded(&self, data_col: usize, value: &SharedString) -> bool {
        self.filters
            .get(data_col)
            .is_some_and(|e| e.contains(value))
    }

    /// Keep exactly `kept` in a column, excluding every other value it has, then renarrow. The
    /// dropdown reports what the user ticked; the delegate stores the complement.
    pub(crate) fn set_column_kept(&mut self, data_col: usize, kept: &[SharedString]) {
        let excluded_now: HashSet<SharedString> = self
            .column_values(data_col)
            .into_iter()
            .filter(|v| !kept.contains(v))
            .collect();
        if let Some(excluded) = self.filters.get_mut(data_col) {
            *excluded = excluded_now;
        }
        self.recompute_visible();
        // The edited cell may have just been filtered out from under the editor.
        self.editing = EditState::Idle;
    }

    /// Re-check every value in a column, then renarrow.
    pub(crate) fn clear_column_filter(&mut self, data_col: usize) {
        if let Some(excluded) = self.filters.get_mut(data_col) {
            excluded.clear();
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    /// Uncheck every value in a column, then renarrow.
    pub(crate) fn exclude_all_in_column(&mut self, data_col: usize) {
        let all = self.column_values(data_col).into_iter().collect();
        if let Some(excluded) = self.filters.get_mut(data_col) {
            *excluded = all;
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    /// Whether a column currently narrows the view — drives the header affordance's active state.
    pub(crate) fn column_has_filter(&self, data_col: usize) -> bool {
        self.filters.get(data_col).is_some_and(|e| !e.is_empty())
    }

    /// Whether a column shows a filter dropdown at all.
    pub(crate) fn column_filter_enabled(&self, data_col: usize) -> bool {
        self.filters_enabled.get(data_col).copied().unwrap_or(false)
    }

    /// Switch a column's filter dropdown on or off. Switching off also clears whatever that
    /// column was excluding, so a hidden filter can't keep narrowing the view invisibly.
    pub fn set_column_filter_enabled(&mut self, data_col: usize, enabled: bool) {
        if let Some(slot) = self.filters_enabled.get_mut(data_col) {
            *slot = enabled;
        }
        if !enabled {
            self.clear_column_filter(data_col);
        }
    }

    /// Apply the saved settings filtering depends on: which columns offer a dropdown, looked up by
    /// each column's key, and the sub-delimiter that splits a cell into values. Takes a closure
    /// rather than the settings type so this crate doesn't depend on its shape.
    pub fn apply_column_settings(
        &mut self,
        filter_enabled: impl Fn(&str) -> bool,
        subdelimiter: SharedString,
    ) {
        let wanted: Vec<bool> = self
            .columns
            .iter()
            .map(|c| filter_enabled(c.key.as_ref()))
            .collect();
        for (data_col, enabled) in wanted.into_iter().enumerate() {
            self.set_column_filter_enabled(data_col, enabled);
        }
        if self.subdelimiter != subdelimiter {
            // What counts as one value just changed, so every stored exclusion is addressed in the
            // old vocabulary. Dropping them beats leaving filters that match nothing.
            self.subdelimiter = subdelimiter;
            for excluded in &mut self.filters {
                excluded.clear();
            }
            self.recompute_visible();
            self.values_generation += 1;
        }
    }

    /// Cells in the *visible* set `re` matches, as `(view_row, data_col)` in view order — ready for
    /// `set_selected_cell`/`scroll_to_row` after the pinned `+1` on the column.
    pub(crate) fn search_matches(
        &self,
        re: &regex::Regex,
        opts: SearchOpts,
    ) -> Vec<(usize, usize)> {
        let mut hits = find_matches(&self.rows, &self.visible_rows, self.columns.len(), re);
        if opts.files {
            hits.extend(find_file_matches(
                &self.rows,
                &self.visible_rows,
                &self.image_paths,
                &self.document_text,
                re,
            ));
            hits.sort_unstable();
            hits.dedup();
        }
        hits
    }

    /// Every filtered row with a linked file, as `(source_row, file)` — what a visual search scores.
    /// Ignores any narrowing a previous search applied.
    pub(crate) fn linked_rows(&self) -> Vec<(usize, PathBuf)> {
        self.filtered_rows
            .iter()
            .filter_map(|&source| Some((source, self.image_paths.get(source)?.clone()?)))
            .collect()
    }

    /// The column of the cell naming a row's linked file, where a hit about that file lands.
    pub(crate) fn file_column(&self, source: usize) -> usize {
        match (self.rows.get(source), self.row_image(source)) {
            (Some(row), Some(path)) => file_column(row, path),
            _ => 0,
        }
    }

    /// The filtered rows `re` hits, in cells or linked files, as source rows in order. Ignores any
    /// narrowing a previous search applied.
    pub(crate) fn hit_rows(&self, re: &regex::Regex, opts: SearchOpts) -> Vec<usize> {
        let mut hits = find_matches(&self.rows, &self.filtered_rows, self.columns.len(), re);
        if opts.files {
            hits.extend(find_file_matches(
                &self.rows,
                &self.filtered_rows,
                &self.image_paths,
                &self.document_text,
                re,
            ));
        }
        let mut rows: Vec<usize> = hits
            .into_iter()
            .map(|(at, _)| self.filtered_rows[at])
            .collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Show only `rows`, in that order, until a later call passes `None`. Column filters still apply.
    /// Returns whether the view changed.
    pub(crate) fn set_search_rows(&mut self, rows: Option<Vec<usize>>) -> bool {
        if self.search_rows == rows {
            return false;
        }
        self.search_rows = rows;
        self.recompute_visible();
        self.editing = EditState::Idle;
        true
    }

    /// Every distinct file the rows link to.
    pub(crate) fn linked_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.image_paths.iter().flatten().cloned().collect();
        files.sort_unstable();
        files.dedup();
        files
    }

    /// Linked documents whose text has not been read yet, each once.
    pub(crate) fn unread_documents(&mut self) -> Vec<PathBuf> {
        let (image_paths, document_text) = (&self.image_paths, &self.document_text);
        self.unread
            .get_or_insert_with(|| {
                let mut unread: Vec<PathBuf> = image_paths
                    .iter()
                    .flatten()
                    .filter(|path| preview::has_text(path) && !document_text.contains_key(*path))
                    .cloned()
                    .collect();
                unread.sort_unstable();
                unread.dedup();
                unread
            })
            .clone()
    }

    pub(crate) fn add_document_text(&mut self, texts: Vec<(PathBuf, String)>) {
        self.document_text.extend(texts);
        let read = &self.document_text;
        if let Some(unread) = &mut self.unread {
            unread.retain(|path| !read.contains_key(path));
        }
    }

    /// The cell writes replacing what `re` matches with `replacement`, in *source* coordinates and
    /// ready for [`apply_edit`](Self::apply_edit). `only` limits it to one `(view_row, data_col)`
    /// hit — Replace vs Replace All.
    pub(crate) fn replace_edits(
        &self,
        re: &regex::Regex,
        replacement: &str,
        opts: SearchOpts,
        only: Option<(usize, usize)>,
    ) -> Cells {
        replace_edits(
            &self.rows,
            &self.visible_rows,
            self.columns.len(),
            re,
            replacement,
            opts,
            only,
        )
    }

    /// Replaces the whole column/row model with real project data. Clears any selection/edit
    /// state, which may index into the old shape.
    pub fn set_data(
        &mut self,
        headers: &[String],
        row_ids: &[settings::project::RowId],
        rows: &[Vec<String>],
    ) {
        self.columns = headers.iter().map(|h| new_column(h.clone())).collect();
        self.rows = rows
            .iter()
            .map(|r| r.iter().map(|c| SharedString::from(c.clone())).collect())
            .collect();
        self.row_ids = row_ids.to_vec();
        self.next_row_id = self
            .row_ids
            .iter()
            .copied()
            .max()
            .and_then(|id| id.checked_add(1))
            .unwrap_or(1);
        self.selection = None;
        self.range = None;
        self.selected_rows.clear();
        self.editing = EditState::Idle;
        // Recorded edits index into the outgoing dataset.
        self.history = History::default();
        self.unsaved.clear();
        self.stamped_history = 0;
        self.saved_history = 0;
        // Saves still writing the outgoing dataset keep the old ledger.
        self.ledger = Arc::default();
        self.filters = vec![HashSet::new(); self.columns.len()];
        self.filters_enabled = vec![false; self.columns.len()];
        self.search_rows = None;
        self.hierarchy = Hierarchy::from_rows(&self.row_ids, &[], &self.default_level);
        self.recompute_visible();
        // Stale — indexes into the old row set; `TablePanel` re-resolves right after.
        self.image_paths = vec![None; self.rows.len()];
        self.images_task = None;
        self.unread = None;
        self.values_generation += 1;
    }

    pub fn set_structure(
        &mut self,
        structure: &[settings::project::RowStructure],
        default_level: &str,
    ) {
        self.default_level = default_level.into();
        self.hierarchy = Hierarchy::from_rows(&self.row_ids, structure, default_level);
        self.recompute_visible();
    }

    pub fn row_structure(&self) -> Structure {
        self.hierarchy.rows()
    }

    /// Replaces the per-row resolved image paths. A length mismatch (a stale call racing a newer
    /// `set_data`) is ignored rather than panicking.
    pub fn set_image_paths(&mut self, paths: Vec<Option<PathBuf>>) {
        if paths.len() == self.rows.len() {
            self.image_paths = paths;
            self.unread = None;
        }
    }

    /// Replace some rows' resolved image paths, as `(source_row, path)`.
    pub(crate) fn set_row_images(&mut self, images: Vec<(usize, Option<PathBuf>)>) {
        for (row, path) in images {
            if let Some(slot) = self.image_paths.get_mut(row) {
                *slot = path;
            }
        }
        self.unread = None;
    }

    /// Every row's cells, and the headers they sit under, as refcounted clones — cheap to take on
    /// the UI thread and hand to a background task.
    pub(crate) fn grid(&self) -> (Vec<SharedString>, Vec<Vec<SharedString>>) {
        let headers = self.columns.iter().map(|c| c.name.clone()).collect();
        (headers, self.rows.clone())
    }

    /// What a save writes: the grid, its row identities, its hierarchy, and the log entries not yet
    /// on disk.
    pub(crate) fn save_snapshot(&self) -> SaveSnapshot {
        let (headers, rows) = self.grid();
        SaveSnapshot {
            headers,
            row_ids: self.row_ids.clone(),
            rows,
            structure: self.hierarchy.rows(),
            history: self.unsaved.clone(),
            first: self.saved_history,
            edits: self.edits,
            ledger: self.ledger.clone(),
        }
    }

    pub(crate) fn edits(&self) -> u64 {
        self.edits
    }

    /// Push in each declared column's type and description, keyed by column name.
    pub(crate) fn set_declared(
        &mut self,
        declared: HashMap<SharedString, (ColumnType, SharedString)>,
    ) {
        self.declared = declared;
    }

    /// A data column's declared type; `Text` for a column nobody typed.
    pub(crate) fn column_type(&self, data_col: usize) -> ColumnType {
        self.columns
            .get(data_col)
            .and_then(|column| self.declared.get(&column.name))
            .map(|(kind, _)| *kind)
            .unwrap_or_default()
    }

    /// A column's description, if it has one.
    pub(crate) fn column_description(&self, name: &str) -> Option<SharedString> {
        self.declared
            .get(name)
            .map(|(_, notes)| notes.clone())
            .filter(|notes| !notes.is_empty())
    }

    /// The selected row's resolved image path, if the files folder had a match for it.
    pub fn row_image(&self, row: usize) -> Option<&Path> {
        self.image_paths.get(row).and_then(|p| p.as_deref())
    }

    /// Cell text at `(row, col)`, if in range. `col` is a data-column index, not shifted for the
    /// pinned row-index column.
    pub fn cell(&self, row: usize, col: usize) -> Option<&SharedString> {
        self.rows.get(row).and_then(|r| r.get(col))
    }

    /// A data column's settings key, which is its header name — the identity `__columns`,
    /// `__notes` and `diagnostics::Location` already address columns by, so an insert or a delete
    /// shifts nothing. Renaming a column is therefore a re-keying, and moves its settings with it.
    pub(crate) fn column_key(&self, data_col: usize) -> SharedString {
        self.columns
            .get(data_col)
            .map(|c| c.key.clone())
            .unwrap_or_default()
    }

    /// Re-run every registered validator over the delegate's own copy of the data.
    ///
    /// The delegate's copy, not `CurrentProject`'s: between a commit and the next save they differ,
    /// and the squiggle has to follow the cell the user just typed into.
    pub(crate) fn revalidate(&self, cx: &mut App) {
        let columns: Vec<(SharedString, SharedString)> = self
            .columns
            .iter()
            .map(|c| (c.key.clone(), c.name.clone()))
            .collect();
        diagnostics::Validators::run(&columns, &self.rows, &self.row_ids, cx);
    }

    /// Every row's text for a data column, in source-row order — the same view a validator gets,
    /// handed to a contributed menu command so it can decide for itself what to do with it.
    /// Unlike [`Self::column_values`], nothing is deduplicated or dropped.
    pub(crate) fn column_cells(&self, data_col: usize) -> Vec<SharedString> {
        self.rows
            .iter()
            .map(|r| r.get(data_col).cloned().unwrap_or_default())
            .collect()
    }

    /// The header text of a data column, for the filter dropdown's title.
    pub(crate) fn column_name(&self, data_col: usize) -> SharedString {
        self.columns
            .get(data_col)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_cell(&mut self, row: usize, col: usize, value: SharedString) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = value;
            self.values_generation += 1;
        }
    }

    /// Whether a cell falls inside the shift-click range. Both arguments are view/display
    /// coordinates, which is what `render_td` already has in hand.
    pub(crate) fn in_range(&self, view_row: usize, col: usize) -> bool {
        let Some(((ar, ac), (hr, hc))) = self.range else {
            return false;
        };
        normalize(ar, hr).contains(&view_row) && normalize(ac, hc).contains(&col)
    }

    /// The range's cells as `(source_rows, cols)` — source rows so writes land on the real data,
    /// in view order so a range over a filtered view covers what the user actually selected.
    ///
    /// With no range drawn this is whatever the selection covers, which is what makes Copy, Cut,
    /// Paste and Clear work on a whole row or column without each of them having its own menu item:
    /// a selected row is every column of it, a selected column every row the filter leaves visible.
    pub(crate) fn range_cells(&self) -> Option<(Vec<usize>, RangeInclusive<usize>)> {
        let Some(((ar, ac), (hr, hc))) = self.range else {
            if self.columns.is_empty() {
                return None;
            }
            // A ⌘-clicked set of rows is every column of each, the same way one selected row is —
            // so Copy, Cut, Paste and Clear widen to a multi-selection without any of them
            // learning that multi-selection exists.
            if !self.selected_rows.is_empty() {
                let rows = self.selected_source_rows();
                return (!rows.is_empty()).then(|| (rows, 0..=self.columns.len() - 1));
            }
            return match self.selection? {
                Selection::Cell { row, col } => Some((vec![row], col..=col)),
                Selection::Row(row) => Some((vec![row], 0..=self.columns.len() - 1)),
                Selection::Column(col) => Some((self.visible_rows.clone(), col..=col)),
            };
        };
        let rows = normalize(ar, hr)
            .filter_map(|view| self.source(view))
            .collect();
        Some((rows, normalize(ac, hc)))
    }

    /// The next `count` source rows at and after `source`, in view order — how far down a paste
    /// taller than the selection can reach. Comes back short at the end of the view: a paste fills
    /// what exists rather than growing the grid.
    pub(crate) fn rows_from(&self, source: usize, count: usize) -> Vec<usize> {
        let Some(view) = self.view_row(source) else {
            return Vec::new();
        };
        self.visible_rows
            .iter()
            .skip(view)
            .take(count)
            .copied()
            .collect()
    }

    /// Write cells and record the whole batch as a single undo step. Every editing path goes
    /// through here — inline commit, diagnostic fix, paste, bulk fill — so none of them carries
    /// undo logic of its own. Cells whose text is unchanged are dropped, so a commit that typed
    /// nothing doesn't consume an undo.
    pub(crate) fn apply_edit(&mut self, cells: Cells, origin: Origin) -> bool {
        let mut edit = Vec::with_capacity(cells.len());
        for (row, col, after) in cells {
            let Some(before) = self.cell(row, col).cloned() else {
                continue;
            };
            if before == after {
                continue;
            }
            self.set_cell(row, col, after.clone());
            edit.push((row, col, before, after));
        }
        let changed = !edit.is_empty();
        self.record(Step::Cells(edit), origin);
        changed
    }

    /// Re-link rows to files: each `(row, col, link)` writes the Filename cell and the row's
    /// `source_path` together, and the whole batch undoes as one step.
    pub(crate) fn relink(&mut self, links: Vec<(usize, usize, SharedString)>, origin: Origin) {
        let before = self.hierarchy.rows();
        let mut edit = Vec::with_capacity(links.len());
        for (row, col, after) in links {
            let Some(before) = self.cell(row, col).cloned() else {
                continue;
            };
            if let Some(id) = self.row_id(row) {
                self.hierarchy.set_source_file(id, after.to_string());
            }
            if before != after {
                self.set_cell(row, col, after.clone());
                edit.push((row, col, before, after));
            }
        }
        let after = self.hierarchy.rows();
        let cells = Step::Cells(edit);
        self.edits += 1;
        let changes = self.changes(&cells);
        self.log(origin, changes);
        self.history.push(
            Step::Batch(vec![cells, Step::Hierarchy { before, after }]),
            self.undo_cap,
        );
    }

    /// Push a step that has just been applied onto the undo stack, and log it.
    fn record(&mut self, step: Step, origin: Origin) {
        self.edits += 1;
        let changes = self.changes(&step);
        self.log(origin, changes);
        self.history.push(step, self.undo_cap);
    }

    fn log(&mut self, origin: Origin, changes: Vec<Change>) {
        if !changes.is_empty() {
            self.unsaved.push(Entry::new(origin, changes, None));
        }
    }

    /// What a step did, by row id and column name. Read against the grid as it stands just after
    /// the step, which is the only state its indices are guaranteed to mean anything in.
    fn changes(&self, step: &Step) -> Vec<Change> {
        let row_id = |row: usize| self.row_ids.get(row).copied().unwrap_or_default();
        let named = |cells: &[SharedString]| {
            self.columns
                .iter()
                .zip(cells)
                .map(|(c, text)| (c.name.to_string(), text.to_string()))
                .collect()
        };
        let by_row = |col: &Col| {
            self.row_ids
                .iter()
                .zip(&col.cells)
                .map(|(id, text)| (*id, text.to_string()))
                .collect()
        };
        match step {
            Step::Cells(cells) => cells
                .iter()
                .map(|(row, col, before, after)| Change::Cell {
                    row: row_id(*row),
                    column: self.column_name(*col).to_string(),
                    before: before.to_string(),
                    after: after.to_string(),
                })
                .collect(),
            Step::RowsAdded {
                at, rows, cells, ..
            } => {
                let mut changes: Vec<_> = rows
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| Change::RowAdded {
                        row: row.id,
                        position: at + offset,
                        cells: named(&row.cells),
                    })
                    .collect();
                changes.extend(cells.iter().map(|(row, col, before, after)| Change::Cell {
                    row: row_id(*row),
                    column: self.column_name(*col).to_string(),
                    before: before.to_string(),
                    after: after.to_string(),
                }));
                changes
            }
            Step::RowsRemoved { rows, .. } => rows
                .iter()
                .map(|(at, row)| Change::RowRemoved {
                    row: row.id,
                    position: *at,
                    cells: named(&row.cells),
                })
                .collect(),
            Step::ColumnAdded { at, col } => vec![Change::ColumnAdded {
                column: col.column.name.to_string(),
                position: *at,
                cells: by_row(col),
            }],
            Step::ColumnRemoved { at, col } => vec![Change::ColumnRemoved {
                column: col.column.name.to_string(),
                position: *at,
                cells: by_row(col),
            }],
            Step::Renamed { before, after, .. } => vec![Change::ColumnRenamed {
                before: before.to_string(),
                after: after.to_string(),
            }],
            Step::ColumnMoved { from, to } => vec![Change::ColumnMoved {
                column: self.column_name(*to).to_string(),
                from: *from,
                to: *to,
            }],
            // A batch's steps are read one by one, each in its own state, as they are replayed.
            Step::Batch(_) | Step::Hierarchy { .. } => Vec::new(),
        }
    }

    /// Reverse the last recorded step, and log that as its own entry. `None` when there was
    /// nothing to undo; otherwise what the undo changed.
    pub(crate) fn undo(&mut self) -> Option<Vec<Change>> {
        let step = self.history.undo()?;
        Some(self.replay_logged(&step, false, Origin::Undo))
    }

    /// [`undo`](Self::undo)'s mirror.
    pub(crate) fn redo(&mut self) -> Option<Vec<Change>> {
        let step = self.history.redo()?;
        Some(self.replay_logged(&step, true, Origin::Redo))
    }

    fn replay_logged(&mut self, step: &Step, forward: bool, origin: Origin) -> Vec<Change> {
        self.edits += 1;
        let mut changes = Vec::new();
        let mut reshaped = Reshaped::default();
        self.replay(step, forward, &mut changes, &mut reshaped);
        self.settle(reshaped);
        self.log(origin, changes.clone());
        changes
    }

    /// Put the grid back the way `changes` say, by identity rather than position: the log's
    /// positions are only where to put a row or column back. Everything applied lands as one undo
    /// step and one log entry. A change that no longer fits — its row already gone, its column
    /// renamed out from under it — is skipped. Notes are not the grid's to apply.
    pub(crate) fn restore(&mut self, changes: &[Change], to: EntryId) -> Vec<Change> {
        self.edits += 1;
        let before = self.hierarchy.rows();
        let mut positions = None;
        let mut steps = Vec::new();
        let mut applied = Vec::new();
        for change in changes {
            match self.apply_change(change, &mut positions) {
                Some(step) => {
                    applied.extend(self.changes(&step));
                    steps.push(step);
                }
                None => log::warn!("restore skipped a change that no longer fits: {change:?}"),
            }
        }
        if steps
            .iter()
            .any(|step| matches!(step, Step::RowsAdded { .. } | Step::RowsRemoved { .. }))
        {
            self.hierarchy.reconcile(&self.row_ids, &self.default_level);
            self.rows_settled();
            let after = self.hierarchy.rows();
            for step in &mut steps {
                if let Step::RowsAdded {
                    before_structure,
                    after_structure,
                    ..
                }
                | Step::RowsRemoved {
                    before_structure,
                    after_structure,
                    ..
                } = step
                {
                    *before_structure = before.clone();
                    *after_structure = after.clone();
                }
            }
        }
        self.log(Origin::Restore(to), applied.clone());
        self.history.push(Step::Batch(steps), self.undo_cap);
        self.editing = EditState::Idle;
        applied
    }

    /// One change applied to the rows raw — [`restore`](Self::restore) settles the hierarchy and
    /// the view once, after the last. `positions` is the id→row map, dropped when rows move.
    fn apply_change(
        &mut self,
        change: &Change,
        positions: &mut Option<HashMap<RowId, usize>>,
    ) -> Option<Step> {
        match change {
            Change::Cell {
                row, column, after, ..
            } => {
                let (r, c) = (
                    position_of(&self.row_ids, positions, *row)?,
                    self.data_col(column)?,
                );
                let before = self.cell(r, c)?.clone();
                let after = SharedString::from(after.clone());
                self.set_cell(r, c, after.clone());
                Some(Step::Cells(vec![(r, c, before, after)]))
            }
            Change::RowAdded {
                row,
                position,
                cells,
            } => {
                if self.row_ids.contains(row) {
                    return None;
                }
                let text = |name: &SharedString| {
                    cells
                        .iter()
                        .find(|(column, _)| column == name.as_ref())
                        .map(|(_, text)| SharedString::from(text.clone()))
                        .unwrap_or_default()
                };
                let rows = vec![Row {
                    id: *row,
                    cells: self.columns.iter().map(|c| text(&c.name)).collect(),
                    image: None,
                }];
                let at = (*position).min(self.rows.len());
                self.splice_rows(at, &rows);
                *positions = None;
                let structure = self.hierarchy.rows();
                Some(Step::RowsAdded {
                    at,
                    rows,
                    cells: Vec::new(),
                    before_structure: structure.clone(),
                    after_structure: structure,
                })
            }
            Change::RowRemoved { row, .. } => {
                let at = position_of(&self.row_ids, positions, *row)?;
                let rows = self.cut_rows(&[at]);
                *positions = None;
                let structure = self.hierarchy.rows();
                Some(Step::RowsRemoved {
                    rows,
                    before_structure: structure.clone(),
                    after_structure: structure,
                })
            }
            Change::ColumnAdded {
                column,
                position,
                cells,
            } => {
                if self.data_col(column).is_some() {
                    return None;
                }
                let col = Col {
                    column: new_column(column.clone()),
                    cells: self
                        .row_ids
                        .iter()
                        .map(|id| {
                            cells
                                .iter()
                                .find(|(row, _)| row == id)
                                .map(|(_, text)| SharedString::from(text.clone()))
                                .unwrap_or_default()
                        })
                        .collect(),
                    excluded: HashSet::new(),
                    filter_enabled: false,
                };
                let at = (*position).min(self.columns.len());
                self.splice_column(at, &col);
                Some(Step::ColumnAdded { at, col })
            }
            Change::ColumnRemoved { column, .. } => {
                let at = self.data_col(column)?;
                let col = self.cut_column(at)?;
                Some(Step::ColumnRemoved { at, col })
            }
            Change::ColumnRenamed { before, after } => {
                let col = self.data_col(before)?;
                if self.data_col(after).is_some() {
                    return None;
                }
                self.set_column_name(col, after.clone().into());
                Some(Step::Renamed {
                    col,
                    before: before.clone().into(),
                    after: after.clone().into(),
                })
            }
            Change::ColumnMoved { column, to, .. } => {
                let from = self.data_col(column)?;
                let to = (*to).min(self.columns.len() - 1);
                self.shift_column(from, to);
                Some(Step::ColumnMoved { from, to })
            }
            Change::Note { .. } => None,
        }
    }

    /// The log entries made since the last save, oldest first.
    pub fn unsaved_history(&self) -> &[Entry] {
        &self.unsaved
    }

    pub(crate) fn stamp_pending_author(&mut self, author: Option<String>) {
        for entry in &mut self.unsaved[self.stamped_history..] {
            entry.author = author.clone();
        }
        self.stamped_history = self.unsaved.len();
    }

    /// Forget the unsaved entries the ledger says have reached the file.
    pub(crate) fn history_written(&mut self) {
        let written = self.ledger.written.load(Ordering::SeqCst);
        let count = (written.saturating_sub(self.saved_history) as usize).min(self.unsaved.len());
        self.unsaved.drain(..count);
        self.saved_history += count as u64;
        self.stamped_history = self.stamped_history.saturating_sub(count);
    }

    /// Apply one side of a recorded step without re-recording it — going through `apply_edit` here
    /// would make undo its own undoable action and the stack would never drain. What it changed is
    /// added to `changes`, each step read in the state it was written against.
    fn replay(
        &mut self,
        step: &Step,
        forward: bool,
        changes: &mut Vec<Change>,
        reshaped: &mut Reshaped,
    ) {
        if !forward {
            changes.extend(self.changes(step).iter().rev().map(Change::inverse));
        }
        match step {
            Step::Cells(cells) => {
                for (row, col, before, after) in cells {
                    let text = if forward { after } else { before };
                    self.set_cell(*row, *col, text.clone());
                }
            }
            Step::RowsAdded {
                at,
                rows,
                cells,
                before_structure,
                after_structure,
            } => {
                if forward {
                    self.splice_rows(*at, rows);
                } else {
                    self.cut_rows(&(*at..at + rows.len()).collect::<Vec<_>>());
                }
                for (row, col, before, after) in cells {
                    let text = if forward { after } else { before };
                    self.set_cell(*row, *col, text.clone());
                }
                reshaped.rows = true;
                reshaped.structure = Some(
                    if forward {
                        after_structure
                    } else {
                        before_structure
                    }
                    .clone(),
                );
            }
            Step::RowsRemoved {
                rows,
                before_structure,
                after_structure,
            } => {
                if forward {
                    self.cut_rows(&rows.iter().map(|(at, _)| *at).collect::<Vec<_>>());
                } else {
                    self.put_back(rows);
                }
                reshaped.rows = true;
                reshaped.structure = Some(
                    if forward {
                        after_structure
                    } else {
                        before_structure
                    }
                    .clone(),
                );
            }
            Step::ColumnAdded { at, col } => {
                if forward {
                    self.splice_column(*at, col);
                } else {
                    let _ = self.cut_column(*at);
                }
            }
            Step::ColumnRemoved { at, col } => {
                if forward {
                    let _ = self.cut_column(*at);
                } else {
                    self.splice_column(*at, col);
                }
            }
            Step::Renamed { col, before, after } => {
                self.set_column_name(*col, if forward { after } else { before }.clone());
            }
            Step::ColumnMoved { from, to } => match forward {
                true => self.shift_column(*from, *to),
                false => self.shift_column(*to, *from),
            },
            Step::Batch(steps) => match forward {
                true => steps
                    .iter()
                    .for_each(|s| self.replay(s, true, changes, reshaped)),
                false => steps
                    .iter()
                    .rev()
                    .for_each(|s| self.replay(s, false, changes, reshaped)),
            },
            Step::Hierarchy { before, after } => {
                reshaped.structure = Some(if forward { after } else { before }.clone());
            }
        }
        if forward {
            changes.extend(self.changes(step));
        }
        // An open edit would commit over what was just restored.
        self.editing = EditState::Idle;
    }

    /// Rebuild what replayed steps invalidated, once for however many there were. The last
    /// structure a step asked for is the one that matches the rows as they now stand.
    fn settle(&mut self, reshaped: Reshaped) {
        match reshaped.structure {
            Some(structure) => {
                self.hierarchy
                    .replace_rows(&self.row_ids, &structure, &self.default_level)
            }
            None if reshaped.rows => self.hierarchy.reconcile(&self.row_ids, &self.default_level),
            None => return,
        }
        match reshaped.rows {
            true => self.rows_settled(),
            false => self.recompute_visible(),
        }
    }

    /// Put `rows` in at `at`, keeping everything row-indexed parallel. Raw: the caller brings the
    /// hierarchy along and then calls [`rows_settled`](Self::rows_settled).
    fn splice_rows(&mut self, at: usize, rows: &[Row]) {
        let at = at.min(self.rows.len());
        self.rows
            .splice(at..at, rows.iter().map(|row| row.cells.clone()));
        self.row_ids.splice(at..at, rows.iter().map(|row| row.id));
        self.image_paths
            .splice(at..at, rows.iter().map(|row| row.image.clone()));
        if let Some(next) = rows
            .iter()
            .map(|row| row.id)
            .max()
            .and_then(|id| id.checked_add(1))
        {
            self.next_row_id = self.next_row_id.max(next);
        }
    }

    /// Put rows cut by [`cut_rows`](Self::cut_rows) back at the positions it handed out, in one
    /// pass. Raw, like [`splice_rows`](Self::splice_rows).
    fn put_back(&mut self, rows: &[(usize, Row)]) {
        let mut kept = std::mem::take(&mut self.rows)
            .into_iter()
            .zip(std::mem::take(&mut self.row_ids))
            .zip(std::mem::take(&mut self.image_paths));
        let mut returning = rows.iter().peekable();
        loop {
            let at = self.rows.len();
            let ((cells, id), image) = match returning.next_if(|(to, _)| *to <= at) {
                Some((_, row)) => ((row.cells.clone(), row.id), row.image.clone()),
                None => match kept.next() {
                    Some(row) => row,
                    None => match returning.next() {
                        Some((_, row)) => ((row.cells.clone(), row.id), row.image.clone()),
                        None => break,
                    },
                },
            };
            self.rows.push(cells);
            self.row_ids.push(id);
            self.image_paths.push(image);
        }
        if let Some(next) = rows
            .iter()
            .map(|(_, row)| row.id)
            .max()
            .and_then(|id| id.checked_add(1))
        {
            self.next_row_id = self.next_row_id.max(next);
        }
    }

    /// Take out the rows at `ats`, in any order, and hand them back ascending — the order they go
    /// back in. Raw, like [`splice_rows`](Self::splice_rows).
    fn cut_rows(&mut self, ats: &[usize]) -> Vec<(usize, Row)> {
        let doomed: BTreeSet<usize> = ats
            .iter()
            .copied()
            .filter(|&r| r < self.rows.len())
            .collect();
        if doomed.is_empty() {
            return Vec::new();
        }
        let mut cut = Vec::with_capacity(doomed.len());
        let rows = std::mem::take(&mut self.rows)
            .into_iter()
            .zip(std::mem::take(&mut self.row_ids))
            .zip(std::mem::take(&mut self.image_paths));
        for (at, ((cells, id), image)) in rows.enumerate() {
            if doomed.contains(&at) {
                cut.push((at, Row { id, cells, image }));
            } else {
                self.rows.push(cells);
                self.row_ids.push(id);
                self.image_paths.push(image);
            }
        }
        cut
    }

    /// Put a column back at `at` exactly as it was. The unrecorded half of a column insert.
    fn splice_column(&mut self, at: usize, col: &Col) {
        let at = at.min(self.columns.len());
        self.columns.insert(at, col.column.clone());
        for (row, cell) in self.rows.iter_mut().zip(&col.cells) {
            row.insert(at.min(row.len()), cell.clone());
        }
        self.filters.insert(at, col.excluded.clone());
        self.filters_enabled.insert(at, col.filter_enabled);
        if at < self.frozen {
            self.frozen += 1;
        }
        self.columns_changed();
    }

    /// [`splice_column`](Self::splice_column)'s inverse.
    fn cut_column(&mut self, at: usize) -> Option<Col> {
        if at >= self.columns.len() {
            return None;
        }
        let col = Col {
            column: self.columns.remove(at),
            cells: self
                .rows
                .iter_mut()
                .map(|row| {
                    if at < row.len() {
                        row.remove(at)
                    } else {
                        SharedString::default()
                    }
                })
                .collect(),
            excluded: self.filters.remove(at),
            filter_enabled: self.filters_enabled.remove(at),
        };
        if at < self.frozen {
            self.frozen -= 1;
        }
        self.columns_changed();
        Some(col)
    }

    /// What every row insert or delete invalidates, once the hierarchy has caught up with the rows.
    /// The view is rebuilt rather than patched: a filtered view's indices all move.
    fn rows_settled(&mut self) {
        // Hits are source rows, which an insert or delete just shifted.
        self.search_rows = None;
        self.recompute_visible();
        self.editing = EditState::Idle;
        self.values_generation += 1;
        self.unread = None;
        self.clamp_selection();
    }

    /// [`rows_settled`](Self::rows_settled)'s column counterpart. Still renarrows: a column that
    /// was hiding rows stops doing so when it goes.
    fn columns_changed(&mut self) {
        self.recompute_visible();
        self.editing = EditState::Idle;
        self.values_generation += 1;
        self.clamp_selection();
    }

    /// Pull the selection back inside the grid it now points past, or drop it if there's no grid
    /// left. A stale selection would put the row-number highlight and the Details panel on a row
    /// that no longer exists.
    fn clamp_selection(&mut self) {
        let (rows, cols) = (self.rows.len(), self.columns.len());
        // Dropped rather than clamped: two selected rows both clamping to the last one would
        // silently merge into one selection, and a delete is exactly when that would mislead.
        self.selected_rows.retain(|&row| row < rows);
        self.selection = match self.selection {
            _ if rows == 0 || cols == 0 => None,
            Some(Selection::Cell { row, col }) => Some(Selection::Cell {
                row: row.min(rows - 1),
                col: col.min(cols - 1),
            }),
            Some(Selection::Row(row)) => Some(Selection::Row(row.min(rows - 1))),
            Some(Selection::Column(col)) => Some(Selection::Column(col.min(cols - 1))),
            None => None,
        };
    }

    /// Insert a blank row at `at`, or a copy of `source` when duplicating a row.
    pub(crate) fn insert_rows(&mut self, at: usize, source: Option<usize>) {
        let before_structure = self.hierarchy.rows();
        let cells = match source.and_then(|r| self.rows.get(r)) {
            Some(row) => row.clone(),
            None => vec![SharedString::default(); self.columns.len()],
        };
        let image = source.and_then(|r| self.image_paths.get(r).cloned().flatten());
        let id = self.fresh_row_id();
        let rows = vec![Row { id, cells, image }];
        self.splice_rows(at, &rows);
        self.hierarchy.reconcile(&self.row_ids, &self.default_level);
        // A duplicate follows its original; a blank row takes the place of the row it pushed down.
        let neighbour = source
            .map(|row| (row, false))
            .or_else(|| (at + 1 < self.rows.len()).then_some((at + 1, true)))
            .or_else(|| at.checked_sub(1).map(|row| (row, false)));
        if let Some((neighbour, before)) = neighbour
            && let Some(neighbour_id) = self.row_id(neighbour)
        {
            let level = self.hierarchy.level(neighbour_id).map(str::to_owned);
            let placement = match before {
                true => Placement::Before(neighbour_id),
                false => Placement::After(neighbour_id),
            };
            match self.hierarchy.move_row(id, placement) {
                Ok(()) => {
                    if let Some(level) = level {
                        let _ = self.hierarchy.set_level(id, &level);
                    }
                }
                Err(error) => log::warn!(
                    "New row {id} was added at the end of the table instead of beside row {}: {error:?}",
                    neighbour + 1
                ),
            }
        }
        self.rows_settled();
        let after_structure = self.hierarchy.rows();
        self.record(
            Step::RowsAdded {
                at,
                rows,
                cells: Vec::new(),
                before_structure,
                after_structure,
            },
            Origin::Structure,
        );
    }

    /// What the project already holds, for [`file_ingest::duplicates::resolve`]: every row's linked
    /// file, by stored source path and by the keys its Filename cell can be looked up under.
    pub(crate) fn existing_components(
        &self,
        file_col: Option<usize>,
        files_root: Option<&Path>,
    ) -> Vec<file_ingest::duplicates::ExistingComponent> {
        let sources: HashMap<_, _> = self
            .hierarchy
            .rows()
            .iter()
            .filter_map(|row| Some((row.row_id, row.source_path.clone()?)))
            .collect();
        self.row_ids
            .iter()
            .enumerate()
            .map(|(source, row_id)| {
                let stored = sources.get(row_id).map(PathBuf::from).map(|path| {
                    match (path.is_absolute(), files_root) {
                        (false, Some(root)) => root.join(path),
                        _ => path,
                    }
                });
                let cell = file_col
                    .and_then(|col| self.rows.get(source)?.get(col))
                    .map(|cell| cell.to_string());
                file_ingest::duplicates::ExistingComponent {
                    key: *row_id as u64,
                    absolute_source: stored.or_else(|| {
                        cell.as_deref()
                            .map(PathBuf::from)
                            .filter(|path| path.is_absolute())
                    }),
                    filename_keys: cell
                        .map(|cell| settings::filenames::lookup_keys(&cell))
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Applies a resolved import as one undo step: new components become rows, and a component the
    /// policy resolved onto an existing row re-links that row rather than copying it.
    pub(crate) fn append_components(
        &mut self,
        resolved: &file_ingest::duplicates::ResolvedPlan,
        title_col: Option<usize>,
        file_col: Option<usize>,
        destination_parent: Option<usize>,
        files_root: Option<&Path>,
    ) -> usize {
        use file_ingest::duplicates::{Parent, Resolution};

        if resolved.plan.components.is_empty() {
            return 0;
        }
        let at = self.rows.len();
        let before_structure = self.hierarchy.rows();
        let destination_parent = destination_parent.and_then(|source| self.row_id(source));
        let mut next_order = before_structure
            .iter()
            .filter(|row| row.parent_id == destination_parent)
            .count() as i64;
        let source_path = |component: &file_ingest::PlannedComponent| {
            file_ingest::normalized_path(
                &files_root
                    .and_then(|root| qrate_export::relative_to(root, &component.absolute_path))
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .unwrap_or_else(|| component.absolute_path.clone()),
            )
        };

        // Row ids first: a component's parent may be resolved onto a row the project already has.
        let mut ids = Vec::with_capacity(resolved.plan.components.len());
        for resolution in &resolved.resolutions {
            let id = match resolution {
                Resolution::Update { existing } | Resolution::Skip { existing } => *existing as i64,
                _ => self.fresh_row_id(),
            };
            ids.push(id);
        }

        let source_of: HashMap<RowId, usize> = self
            .row_ids
            .iter()
            .enumerate()
            .map(|(source, id)| (*id, source))
            .collect();
        let mut after_structure = before_structure.to_vec();
        let slot_of: HashMap<RowId, usize> = after_structure
            .iter()
            .enumerate()
            .map(|(slot, row)| (row.row_id, slot))
            .collect();
        let mut rows = Vec::new();
        let mut cells = Vec::new();
        for (index, component) in resolved.plan.components.iter().enumerate() {
            let is_file = component.kind == file_ingest::EntryKind::File;
            let parent_id = match resolved.parents[index] {
                Some(Parent::Planned(parent)) => ids.get(parent).copied(),
                Some(Parent::Existing(existing)) => Some(existing as i64),
                None => destination_parent,
            };
            match &resolved.resolutions[index] {
                Resolution::Skip { .. } => continue,
                Resolution::Update { existing } => {
                    let existing = *existing as RowId;
                    let Some(&source) = source_of.get(&existing) else {
                        continue;
                    };
                    // Only the link moves. The metadata on this row is the archivist's.
                    if is_file
                        && let Some(col) = file_col
                        && let Some(before) = self.rows.get(source).and_then(|row| row.get(col))
                    {
                        let after = SharedString::from(source_path(component));
                        if *before != after {
                            cells.push((source, col, before.clone(), after));
                        }
                    }
                    if let Some(row) = slot_of
                        .get(&existing)
                        .and_then(|&slot| after_structure.get_mut(slot))
                    {
                        row.source_path = Some(source_path(component));
                    }
                }
                Resolution::Create | Resolution::Ambiguous { .. } => {
                    let mut new_cells = vec![SharedString::default(); self.columns.len()];
                    if let Some(col) = title_col.filter(|col| *col < new_cells.len()) {
                        new_cells[col] = component.title.clone().into();
                    }
                    if is_file && let Some(col) = file_col.filter(|col| *col < new_cells.len()) {
                        new_cells[col] = source_path(component).into();
                    }
                    rows.push(Row {
                        id: ids[index],
                        cells: new_cells,
                        image: is_file.then(|| component.absolute_path.clone()),
                    });
                    let order = match component.parent {
                        Some(_) => index as i64,
                        None => {
                            next_order += 1;
                            next_order - 1
                        }
                    };
                    after_structure.push(settings::project::RowStructure {
                        row_id: ids[index],
                        parent_id,
                        level_key: component.level_key.clone(),
                        sibling_order: order,
                        source_path: Some(source_path(component)),
                        source_kind: Some(match component.kind {
                            file_ingest::EntryKind::File => settings::project::SourceKind::File,
                            file_ingest::EntryKind::Directory => {
                                settings::project::SourceKind::Directory
                            }
                        }),
                    });
                }
            }
        }

        let added = rows.len();
        self.splice_rows(at, &rows);
        for (row, col, _, after) in &cells {
            self.set_cell(*row, *col, after.clone());
        }
        self.hierarchy
            .replace_rows(&self.row_ids, &after_structure, &self.default_level);
        self.rows_settled();
        let after_structure = self.hierarchy.rows();
        self.record(
            Step::RowsAdded {
                at,
                rows,
                cells,
                before_structure,
                after_structure,
            },
            Origin::Structure,
        );
        added
    }

    /// Append spreadsheet values as one undoable step, preserving the project's existing columns.
    /// The new rows join the end of the roots, which is what reconciling the hierarchy does.
    pub(crate) fn append_spreadsheet_rows(&mut self, values: Vec<Vec<SharedString>>) -> usize {
        if values.is_empty() {
            return 0;
        }
        let at = self.rows.len();
        let before_structure = self.hierarchy.rows();
        let rows: Vec<_> = values
            .into_iter()
            .map(|cells| Row {
                id: self.fresh_row_id(),
                cells,
                image: None,
            })
            .collect();
        let added = rows.len();
        self.splice_rows(at, &rows);
        self.hierarchy.reconcile(&self.row_ids, &self.default_level);
        self.rows_settled();
        let after_structure = self.hierarchy.rows();
        self.record(
            Step::RowsAdded {
                at,
                rows,
                cells: Vec::new(),
                before_structure,
                after_structure,
            },
            Origin::Import,
        );
        added
    }

    /// Delete the rows at `ats` as one undo step, carrying their cells and photos on it. The
    /// hierarchy loses them all in one pass.
    pub(crate) fn remove_rows(&mut self, ats: &[usize]) {
        let before_structure = self.hierarchy.rows();
        let removed_ids: Vec<_> = ats.iter().filter_map(|at| self.row_id(*at)).collect();
        self.hierarchy.delete(&removed_ids);
        let removed = self.cut_rows(ats);
        self.rows_settled();
        let after_structure = self.hierarchy.rows();
        self.record(
            Step::RowsRemoved {
                rows: removed,
                before_structure,
                after_structure,
            },
            Origin::Structure,
        );
    }

    pub(crate) fn row_id(&self, source: usize) -> Option<settings::project::RowId> {
        self.row_ids.get(source).copied()
    }

    pub fn row_ids(&self) -> &[settings::project::RowId] {
        &self.row_ids
    }

    pub(crate) fn indent_row(&mut self, source: usize) -> Result<(), crate::hierarchy::Error> {
        let row_id = self
            .row_ids
            .get(source)
            .copied()
            .ok_or(crate::hierarchy::Error::UnknownRow(source as i64))?;
        self.edit_hierarchy(|hierarchy| hierarchy.indent(row_id))
    }

    pub(crate) fn outdent_row(&mut self, source: usize) -> Result<(), crate::hierarchy::Error> {
        let row_id = self
            .row_ids
            .get(source)
            .copied()
            .ok_or(crate::hierarchy::Error::UnknownRow(source as i64))?;
        self.edit_hierarchy(|hierarchy| hierarchy.outdent(row_id))
    }

    pub(crate) fn reparent_row(
        &mut self,
        source: usize,
        parent_source: Option<usize>,
    ) -> Result<(), crate::hierarchy::Error> {
        let row_id = self
            .row_ids
            .get(source)
            .copied()
            .ok_or(crate::hierarchy::Error::UnknownRow(source as i64))?;
        let placement = match parent_source {
            Some(parent) => Placement::ChildOf(
                self.row_ids
                    .get(parent)
                    .copied()
                    .ok_or(crate::hierarchy::Error::UnknownRow(parent as i64))?,
            ),
            None => Placement::Root,
        };
        self.edit_hierarchy(|hierarchy| hierarchy.move_row(row_id, placement))
    }

    pub(crate) fn move_row_relative(
        &mut self,
        source: usize,
        target: usize,
        placement: crate::RowPlacement,
    ) -> Result<(), crate::hierarchy::Error> {
        let row_id = self
            .row_id(source)
            .ok_or(crate::hierarchy::Error::UnknownRow(source as i64))?;
        let target_id = self
            .row_id(target)
            .ok_or(crate::hierarchy::Error::UnknownRow(target as i64))?;
        let placement = match placement {
            crate::RowPlacement::Before => Placement::Before(target_id),
            crate::RowPlacement::Child => Placement::ChildOf(target_id),
            crate::RowPlacement::After => Placement::After(target_id),
        };
        self.edit_hierarchy(|hierarchy| hierarchy.move_row(row_id, placement))
    }

    /// Wrap `rows` in a new blank parent that takes the first one's place, as one undo step. A row
    /// whose ancestor is also picked travels with that ancestor rather than being pulled out of it.
    pub(crate) fn group_rows(&mut self, rows: &[usize]) -> Result<(), crate::hierarchy::Error> {
        let Some(&first) = rows.first() else {
            return Ok(());
        };
        let picked = rows
            .iter()
            .map(|&row| {
                self.row_id(row)
                    .ok_or(crate::hierarchy::Error::UnknownRow(row as i64))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let nested: HashSet<_> = picked
            .iter()
            .filter_map(|&id| self.hierarchy.subtree(id).ok())
            .flat_map(|subtree| subtree.into_iter().skip(1))
            .collect();
        self.insert_rows(first, None);
        let group = self.row_ids[first];
        for id in picked.into_iter().filter(|id| !nested.contains(id)) {
            // A fresh, childless parent cannot be a descendant of anything picked.
            if let Err(error) = self.hierarchy.move_row(id, Placement::ChildOf(group)) {
                log::warn!(
                    "Row {id} stayed where it was instead of joining the new group: {error:?}"
                );
            }
        }
        self.hierarchy.set_expanded(group, true);
        let structure = self.hierarchy.rows();
        if let Some(Step::RowsAdded {
            after_structure, ..
        }) = self.history.last_mut()
        {
            *after_structure = structure;
        }
        self.recompute_visible();
        Ok(())
    }

    pub(crate) fn subtree_sources(&self, source: usize) -> Vec<usize> {
        let Some(row_id) = self.row_ids.get(source).copied() else {
            return Vec::new();
        };
        let Ok(subtree) = self.hierarchy.subtree(row_id) else {
            return Vec::new();
        };
        let subtree: HashSet<_> = subtree.into_iter().collect();
        self.row_ids
            .iter()
            .enumerate()
            .filter_map(|(source, row_id)| subtree.contains(row_id).then_some(source))
            .collect()
    }

    fn edit_hierarchy(
        &mut self,
        edit: impl FnOnce(&mut Hierarchy) -> Result<(), crate::hierarchy::Error>,
    ) -> Result<(), crate::hierarchy::Error> {
        let before = self.hierarchy.rows();
        edit(&mut self.hierarchy)?;
        let after = self.hierarchy.rows();
        self.record(Step::Hierarchy { before, after }, Origin::Structure);
        self.recompute_visible();
        Ok(())
    }

    /// `next_row_id` stays above every id in the grid (`set_data` and `splice_rows` keep it there),
    /// so only a wrapped counter has to look for a free id.
    fn fresh_row_id(&mut self) -> RowId {
        let id = self.next_row_id;
        if let Some(next) = id.checked_add(1) {
            self.next_row_id = next;
            return id;
        }
        let used: HashSet<RowId> = self.row_ids.iter().copied().collect();
        (1..RowId::MAX).find(|id| !used.contains(id)).unwrap_or(id)
    }

    /// Add a blank column at `at`, named for the user to rename. Returns its name.
    pub(crate) fn insert_column(&mut self, at: usize) -> SharedString {
        let name = self.fresh_column_name();
        let col = Col {
            column: new_column(name.clone()),
            cells: vec![SharedString::default(); self.rows.len()],
            excluded: HashSet::new(),
            filter_enabled: false,
        };
        self.splice_column(at, &col);
        self.record(Step::ColumnAdded { at, col }, Origin::Structure);
        name
    }

    /// Delete a column and every row's cell in it, as one undo step.
    pub(crate) fn remove_column(&mut self, at: usize) {
        let Some(col) = self.cut_column(at) else {
            return;
        };
        self.record(Step::ColumnRemoved { at, col }, Origin::Structure);
    }

    /// Rename a column, which re-keys it — the caller moves its stored settings to match. `false`
    /// if the name is blank or another column already answers to it, since a name is an identity.
    pub(crate) fn rename_column(&mut self, col: usize, name: SharedString) -> bool {
        let Some(before) = self.columns.get(col).map(|c| c.name.clone()) else {
            return false;
        };
        if name.trim().is_empty() || self.data_col(&name).is_some_and(|c| c != col) {
            return false;
        }
        self.set_column_name(col, name.clone());
        self.record(
            Step::Renamed {
                col,
                before,
                after: name,
            },
            Origin::Structure,
        );
        true
    }

    /// Move the column at `from` to `to`, its cells and filter with it; row order is untouched.
    fn shift_column(&mut self, from: usize, to: usize) {
        if from >= self.columns.len() || to >= self.columns.len() {
            return;
        }
        let col = self.columns.remove(from);
        self.columns.insert(to, col);
        move_col(&mut self.rows, from, to);
        self.values_generation += 1;
        if from < self.filters.len() && to < self.filters.len() {
            let f = self.filters.remove(from);
            self.filters.insert(to, f);
        }
        if from < self.filters_enabled.len() && to < self.filters_enabled.len() {
            let e = self.filters_enabled.remove(from);
            self.filters_enabled.insert(to, e);
        }
        // An in-flight edit and the shift-click range both index into the old column order.
        self.editing = EditState::Idle;
        self.range = None;
    }

    /// The name *and* the key, which are the same string — see [`Self::column_key`].
    fn set_column_name(&mut self, col: usize, name: SharedString) {
        if let Some(column) = self.columns.get_mut(col) {
            column.key = name.clone();
            column.name = name;
        }
    }

    /// A name no column answers to yet: `Column 4`, else `Column 5`, and so on.
    fn fresh_column_name(&self) -> SharedString {
        (self.columns.len() + 1..)
            .map(|n| SharedString::from(format!("Column {n}")))
            .find(|name| self.data_col(name).is_none())
            .unwrap_or_default()
    }

    /// The current selection — a cell, a whole row, or a whole column.
    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    /// Every selected item as source rows, in view order: the ⌘-clicked set unioned with whatever
    /// row the cursor is on. This is what Details, the gallery, the selection menu and the status
    /// bar all count — one answer to "what is selected", so none of them can disagree.
    ///
    /// Filtered-away rows stay in the set but drop out here, so an action reaches what the
    /// archivist can actually see and clearing the filter brings the rest back.
    pub fn selected_source_rows(&self) -> Vec<usize> {
        let cursor = match self.selection {
            Some(Selection::Cell { row, .. } | Selection::Row(row)) => Some(row),
            Some(Selection::Column(_)) | None => None,
        };
        self.visible_rows
            .iter()
            .copied()
            .filter(|row| self.selected_rows.contains(row) || cursor == Some(*row))
            .collect()
    }

    /// Whether this source row is part of a multi-selection — the cheap per-row test the grid and
    /// the gallery run while rendering, without building the vector above.
    pub fn is_row_selected(&self, row: usize) -> bool {
        self.selected_rows.contains(&row)
            || matches!(
                self.selection,
                Some(Selection::Cell { row: r, .. } | Selection::Row(r)) if r == row
            )
    }

    /// Add or remove one row from the multi-selection — ⌘-click. The cursor is folded into the set
    /// first, so the row already selected when the modifier goes down doesn't vanish.
    pub fn toggle_row(&mut self, row: usize) {
        if let Some(Selection::Cell { row: r, .. } | Selection::Row(r)) = self.selection {
            self.selected_rows.insert(r);
        }
        self.range = None;
        if self.selected_rows.remove(&row) {
            // Leaving the cursor on the row just removed would count it as selected again — the
            // cursor is part of the selection, so it has to retreat to whatever is still picked.
            self.selection = self
                .selected_rows
                .iter()
                .next_back()
                .map(|&r| Selection::Row(r));
            return;
        }
        self.selected_rows.insert(row);
        self.selection = Some(Selection::Row(row));
    }

    /// Select every visible row between the cursor and `row` — shift-click down the `#` column.
    pub fn extend_rows_to(&mut self, row: usize) {
        let anchor = match self.selection {
            Some(Selection::Cell { row: r, .. } | Selection::Row(r)) => r,
            Some(Selection::Column(_)) | None => row,
        };
        let (Some(from), Some(to)) = (self.view_row(anchor), self.view_row(row)) else {
            return;
        };
        let run: Vec<_> = normalize(from, to)
            .filter_map(|view| self.source(view))
            .collect();
        self.selected_rows.extend(run);
        self.range = None;
        self.selection = Some(Selection::Row(row));
    }

    /// Deselect everything.
    pub fn clear_selection(&mut self) {
        self.selected_rows.clear();
        self.range = None;
        self.selection = None;
    }

    /// Collapse back to a single row — what a plain click leaves.
    pub fn select_only_row(&mut self, row: usize) {
        self.selected_rows.clear();
        self.range = None;
        self.selection = Some(Selection::Row(row));
    }

    /// The `(source_row, data_col)` whose row-number and column header should be highlighted
    /// (Google-Sheets style): the cell being edited, else the selected cell. `None` when nothing
    /// cell-specific is active (a whole-row/column selection, or none).
    pub(crate) fn active_cell(&self) -> Option<(usize, usize)> {
        if let EditState::Editing { row, col } = self.editing {
            return Some((row, col));
        }
        match self.selection {
            Some(Selection::Cell { row, col }) => Some((row, col)),
            _ => None,
        }
    }

    /// The row's cells as `(column header, cell text)` pairs, in display order — what the
    /// Details panel shows.
    pub fn row_fields(&self, row: usize) -> Vec<(SharedString, SharedString)> {
        self.columns
            .iter()
            .enumerate()
            .map(|(c, col)| {
                (
                    col.name.clone(),
                    self.cell(row, c).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Headers + rows in display order, stringified for the `.qrate` writer and every export.
    /// `save_dataset` drops and recreates `dataset_main`, so the stored physical order is free to
    /// follow the display order rather than having to be recovered from the column keys.
    pub fn dataset_snapshot(&self) -> DatasetSnapshot {
        let headers = self.columns.iter().map(|c| c.name.to_string()).collect();
        let rows = self
            .rows
            .iter()
            .map(|row| {
                (0..self.columns.len())
                    .map(|i| row.get(i).map(|c| c.to_string()).unwrap_or_default())
                    .collect()
            })
            .collect();
        (headers, self.row_ids.clone(), rows)
    }

    /// Current column layout (keys + widths in display order) for persistence.
    pub(crate) fn column_layout(&self) -> ColumnLayout {
        ColumnLayout {
            keys: self.columns.iter().map(|c| c.key.to_string()).collect(),
            widths: self.columns.iter().map(|c| c.width.as_f32()).collect(),
        }
    }

    /// Update column widths from a `ColumnWidthsChanged` event. Index 0 is the pinned row-index
    /// column — skipped.
    pub(crate) fn set_column_widths(&mut self, widths: &[Pixels]) {
        for (col, w) in self.columns.iter_mut().zip(widths.iter().skip(1)) {
            col.width = *w;
        }
    }

    /// Apply a saved layout: reorder columns (and every row's cells, keeping lookups positional)
    /// and set widths. Ignored unless the saved keys are a permutation of the current ones.
    pub(crate) fn apply_column_layout(&mut self, layout: &ColumnLayout) {
        if layout.keys.len() != self.columns.len() || layout.widths.len() != layout.keys.len() {
            return;
        }
        let Some(perm) = layout
            .keys
            .iter()
            .map(|k| self.columns.iter().position(|c| c.key.as_ref() == k))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let mut seen = vec![false; perm.len()];
        for &i in &perm {
            if seen[i] {
                return;
            }
            seen[i] = true;
        }

        self.columns = perm.iter().map(|&i| self.columns[i].clone()).collect();
        for (col, w) in self.columns.iter_mut().zip(&layout.widths) {
            col.width = px(*w);
        }
        for row in &mut self.rows {
            if row.len() == perm.len() {
                *row = perm.iter().map(|&i| row[i].clone()).collect();
            }
        }
        // A column now holds a different column's values.
        self.values_generation += 1;
        // Row order is untouched, so `visible_rows` stays valid — no recompute needed.
        if self.filters.len() == perm.len() {
            self.filters = perm.iter().map(|&i| self.filters[i].clone()).collect();
        }
        if self.filters_enabled.len() == perm.len() {
            self.filters_enabled = perm.iter().map(|&i| self.filters_enabled[i]).collect();
        }
    }
}

/// What replayed steps left for [`QrateTableDelegate::settle`] to rebuild once.
#[derive(Default)]
struct Reshaped {
    rows: bool,
    structure: Option<Structure>,
}

/// Where `row` sits in `row_ids`, through a map built on first ask and dropped by the caller
/// whenever rows move.
fn position_of(
    row_ids: &[RowId],
    positions: &mut Option<HashMap<RowId, usize>>,
    row: RowId,
) -> Option<usize> {
    positions
        .get_or_insert_with(|| {
            row_ids
                .iter()
                .enumerate()
                .map(|(at, id)| (*id, at))
                .collect()
        })
        .get(&row)
        .copied()
}

/// `candidates` as `(score, source_row)`, best first, dropping the ones `score` cannot rate.
pub(crate) fn rank(
    candidates: &[(usize, PathBuf)],
    score: impl Fn(&Path) -> Option<f32>,
) -> Vec<(f32, usize)> {
    let mut scored: Vec<(f32, usize)> = candidates
        .iter()
        .filter_map(|(source, path)| Some((score(path)?, *source)))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
}

/// An inclusive range between two endpoints given in either order — a drag up-and-left selects the
/// same rectangle as the same drag down-and-right.
fn normalize(a: usize, b: usize) -> RangeInclusive<usize> {
    a.min(b)..=a.max(b)
}

/// Move every row's cell at `from` to position `to`, mirroring a column move so cell lookups
/// stay positional (display order == storage order).
fn move_col(rows: &mut [Vec<SharedString>], from: usize, to: usize) {
    for row in rows {
        if from < row.len() && to < row.len() {
            let cell = row.remove(from);
            row.insert(to, cell);
        }
    }
}

/// A fresh grid column under `name`, which is both its label and its key.
fn new_column(name: impl Into<SharedString>) -> Column {
    let name = name.into();
    Column::new(name.clone(), name)
        .width(px(120.))
        .resizable(true)
        .movable(true)
}

/// A cell's filterable values: the whole cell, or its sub-delimited parts when a delimiter is set.
/// Parts are trimmed, because `Film; Video` is written with a space that nobody means as part of
/// the value.
fn cell_parts<'a>(value: &'a SharedString, subdelimiter: &str) -> Vec<&'a str> {
    if subdelimiter.is_empty() {
        return vec![value.as_ref()];
    }
    value.split(subdelimiter).map(str::trim).collect()
}

/// Source-row indices that pass every column's excluded-value filter, in order. Split out of the
/// delegate (which needs a live gpui `App`) so the logic is unit-testable on plain data.
///
/// A multi-valued cell survives while *any* of its values is still checked: unchecking `Film`
/// hides the rows that are only Film, not everything that is also Video.
fn compute_visible_rows(
    rows: &[Vec<SharedString>],
    filters: &[HashSet<SharedString>],
    subdelimiter: &str,
) -> Vec<usize> {
    if filters.iter().all(HashSet::is_empty) {
        return (0..rows.len()).collect();
    }
    (0..rows.len())
        .filter(|&r| {
            filters.iter().enumerate().all(|(c, excluded)| {
                excluded.is_empty()
                    || rows[r].get(c).is_none_or(|v| {
                        cell_parts(v, subdelimiter)
                            .into_iter()
                            .any(|part| !excluded.contains(part))
                    })
            })
        })
        .collect()
}

/// Cells matching `needle` (case-insensitive substring) within the visible set, as
/// `(view_row, data_col)` in view order. `cols` bounds the data columns scanned.
/// The find bar's three toggles, mirroring Zed's search: match case, whole word, and treat the
/// query as a regular expression.
#[derive(Clone, Copy, Default)]
pub(crate) struct SearchOpts {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
    /// Also match the text inside each row's linked document.
    pub files: bool,
    /// Rank rows by how well their linked file matches a description instead of matching text.
    pub visual: bool,
}

/// Compile the query into a matcher honoring the toggles. `None` means "match nothing": a blank
/// query, or (in regex mode) a pattern that fails to parse.
pub(crate) fn compile_search(needle: &str, opts: SearchOpts) -> Option<regex::Regex> {
    if needle.trim().is_empty() {
        return None;
    }
    let body = if opts.regex {
        needle.to_string()
    } else {
        regex::escape(needle)
    };
    let body = if opts.word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    let pattern = if opts.case {
        body
    } else {
        format!("(?i){body}")
    };
    regex::Regex::new(&pattern).ok()
}

fn find_matches(
    rows: &[Vec<SharedString>],
    visible: &[usize],
    cols: usize,
    re: &regex::Regex,
) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    for (view, &source) in visible.iter().enumerate() {
        for col in 0..cols {
            if rows
                .get(source)
                .and_then(|r| r.get(col))
                .is_some_and(|c| re.is_match(c))
            {
                hits.push((view, col));
            }
        }
    }
    hits
}

/// Rows whose linked document `re` matches, addressed at the cell that names the file so
/// stepping to the hit lands where the archivist would look for it.
fn find_file_matches(
    rows: &[Vec<SharedString>],
    visible: &[usize],
    image_paths: &[Option<PathBuf>],
    document_text: &HashMap<PathBuf, String>,
    re: &regex::Regex,
) -> Vec<(usize, usize)> {
    visible
        .iter()
        .enumerate()
        .filter_map(|(view, &source)| {
            let path = image_paths.get(source)?.as_ref()?;
            if !re.is_match(document_text.get(path)?) {
                return None;
            }
            Some((view, file_column(rows.get(source)?, path)))
        })
        .collect()
}

/// The column of the cell in `row` that names `path`, so a hit about the file lands where the
/// archivist would look for it. The first column when no cell names it directly.
fn file_column(row: &[SharedString], path: &Path) -> usize {
    let names = path
        .file_name()
        .map(|name| settings::filenames::keys(&name.to_string_lossy()))
        .unwrap_or_default();
    row.iter()
        .position(|cell| {
            settings::filenames::lookup_keys(cell)
                .iter()
                .any(|key| names.contains(key))
        })
        .unwrap_or(0)
}

/// Rewrite matches in place: `Regex::replace_all` substitutes only the matched spans, so the rest
/// of the cell's text survives and a cell with two occurrences gets both. Literal mode wraps the
/// replacement in `NoExpand` so a `$` the user typed stays a `$`; regex mode leaves `$1` capture
/// references working, which is the point of turning regex on.
fn replace_edits(
    rows: &[Vec<SharedString>],
    visible: &[usize],
    cols: usize,
    re: &regex::Regex,
    replacement: &str,
    opts: SearchOpts,
    only: Option<(usize, usize)>,
) -> Cells {
    let mut edits = Cells::new();
    for (view, &source) in visible.iter().enumerate() {
        for col in 0..cols {
            if only.is_some_and(|hit| hit != (view, col)) {
                continue;
            }
            let Some(before) = rows.get(source).and_then(|r| r.get(col)) else {
                continue;
            };
            let after = if opts.regex {
                re.replace_all(before, replacement)
            } else {
                re.replace_all(before, regex::NoExpand(replacement))
            };
            if after != before.as_ref() {
                edits.push((source, col, after.into_owned().into()));
            }
        }
    }
    edits
}

impl TableDelegate for QrateTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        // +1 for the pinned row-index column at table column 0.
        self.columns.len() + 1
    }

    fn rows_count(&self, _cx: &App) -> usize {
        // The library asks about the *view* — the filtered set — not the source rows.
        self.visible_rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if col_ix == row_index::COL_IX {
            return row_index::column();
        }
        let column = self.columns[col_ix - 1].clone();
        // The library's fixed region is however many leading columns carry this, so a count is the
        // whole of the freezing feature.
        if col_ix - 1 < self.frozen {
            column.fixed_left()
        } else {
            column
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // The pinned `#` column has no filter; render its (blank) name like the default.
        if col_ix == row_index::COL_IX {
            div()
                .child(self.column(col_ix, cx).name.clone())
                .into_any_element()
        } else {
            filter::render_th(self, col_ix - 1, window, cx)
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // `row_ix` is a VIEW index; map it to the source row before touching row-indexed data.
        let Some(source) = self.source(row_ix) else {
            return div().into_any_element();
        };
        if col_ix == row_index::COL_IX {
            // The source row number — the row's stable identity, not its view position. Highlight
            // it while its row holds the active (edited/selected) cell.
            let highlighted = self.active_cell().is_some_and(|(r, _)| r == source)
                || self.selected_rows.contains(&source);
            row_index::render_td(self, row_ix, source, highlighted, cx)
        } else {
            // A ⌘-clicked row paints across every column, which is what makes a discontiguous
            // selection read as a set of *items* rather than a column of tinted cells.
            let ranged = self.in_range(row_ix, col_ix - 1) || self.selected_rows.contains(&source);
            cell::render_cell(self, source, col_ix - 1, ranged, window, cx)
        }
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        // Table indices include the pinned row-index column at 0, which never moves.
        if col_ix == row_index::COL_IX || to_ix == row_index::COL_IX {
            return;
        }
        let (from, to) = (col_ix - 1, to_ix - 1);
        if from >= self.columns.len() || to >= self.columns.len() {
            return;
        }
        self.shift_column(from, to);
        self.record(Step::ColumnMoved { from, to }, Origin::Structure);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(rows: &[&[&str]]) -> Vec<Vec<SharedString>> {
        rows.iter()
            .map(|r| {
                r.iter()
                    .map(|c| SharedString::from(c.to_string()))
                    .collect()
            })
            .collect()
    }

    /// One excluded set per column; each inner slice lists that column's excluded values.
    fn re(needle: &str, opts: SearchOpts) -> regex::Regex {
        compile_search(needle, opts).expect("the test query compiles")
    }

    fn filters(cols: &[&[&str]]) -> Vec<HashSet<SharedString>> {
        cols.iter()
            .map(|vals| {
                vals.iter()
                    .map(|v| SharedString::from(v.to_string()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn move_col_shifts_every_row() {
        let mut g = rows(&[&["a", "b", "c"], &["d", "e", "f"]]);
        move_col(&mut g, 0, 2);
        assert_eq!(g[0], rows(&[&["b", "c", "a"]])[0]);
        assert_eq!(g[1], rows(&[&["e", "f", "d"]])[0]);
    }

    #[test]
    fn move_col_out_of_range_is_noop() {
        let mut g = rows(&[&["a", "b"]]);
        move_col(&mut g, 0, 5);
        assert_eq!(g[0], rows(&[&["a", "b"]])[0]);
    }

    #[test]
    fn no_filter_is_identity_view() {
        let data = rows(&[&["a", "x"], &["b", "y"], &["c", "z"]]);
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&[], &[]]), ""),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn excluding_a_value_hides_its_rows() {
        let data = rows(&[&["a", "x"], &["b", "y"], &["a", "z"]]);
        // Exclude "a" in column 0 → only the middle row (source 1) survives.
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&["a"], &[]]), ""),
            vec![1]
        );
    }

    #[test]
    fn filters_across_columns_are_anded() {
        let data = rows(&[&["a", "x"], &["b", "x"], &["b", "y"]]);
        // Exclude "a" in col 0 and "y" in col 1 → source 0 and source 2 hidden.
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&["a"], &["y"]]), ""),
            vec![1]
        );
    }

    /// The point of the sub-delimiter: `Film; Video` is two subjects. Unchecking one must not take
    /// the row with it while the other is still checked, or a multi-valued column can only ever be
    /// filtered down to nothing.
    #[test]
    fn a_sub_delimited_cell_survives_while_any_of_its_values_is_checked() {
        let data = rows(&[&["Film; Video"], &["Film"], &["Photograph"]]);
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&["Film"]]), ";"),
            vec![0, 2],
            "the Film-only row goes; Film; Video stays for its Video half"
        );
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&["Film", "Video"]]), ";"),
            vec![2]
        );
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&["Film"]]), ""),
            vec![0, 2],
            "with no delimiter the combined cell is its own opaque value"
        );
    }

    #[test]
    fn cell_parts_trims_and_falls_back_to_the_whole_cell() {
        let value = SharedString::from("Film; Video");
        assert_eq!(cell_parts(&value, ";"), vec!["Film", "Video"]);
        assert_eq!(cell_parts(&value, ""), vec!["Film; Video"]);
        assert_eq!(cell_parts(&value, "|"), vec!["Film; Video"]);
    }

    #[test]
    fn find_is_case_insensitive_and_scoped_to_visible() {
        let data = rows(&[
            &["Apple", "red"],
            &["banana", "yellow"],
            &["apricot", "red"],
        ]);
        let all = compute_visible_rows(&data, &filters(&[&[], &[]]), "");
        let opts = SearchOpts::default();
        assert_eq!(
            find_matches(&data, &all, 2, &re("AP", opts)),
            vec![(0, 0), (2, 0)]
        );
        // Hide the "apricot" row: the search must not report it, and the surviving hit is
        // addressed by its *view* row, not its source row.
        let visible = compute_visible_rows(&data, &filters(&[&["apricot"], &[]]), "");
        assert_eq!(visible, vec![0, 1]);
        assert_eq!(
            find_matches(&data, &visible, 2, &re("ap", opts)),
            vec![(0, 0)]
        );
    }

    /// A hit inside a linked PDF lands on the cell naming the file; a row whose document lacks the
    /// query, or that has no document at all, is not reported.
    #[test]
    fn find_reaches_into_linked_documents() {
        let data = rows(&[
            &["Letter to council", "1920_letter.pdf"],
            &["Portrait", "1921_portrait.jpg"],
            &["Minutes", "1922_minutes.pdf"],
        ]);
        let paths = vec![
            Some(PathBuf::from("files/1920_letter.pdf")),
            Some(PathBuf::from("files/1921_portrait.jpg")),
            Some(PathBuf::from("files/1922_minutes.pdf")),
        ];
        let text = HashMap::from([
            (
                PathBuf::from("files/1920_letter.pdf"),
                "the sawmill on Kitsilano beach".to_string(),
            ),
            (
                PathBuf::from("files/1922_minutes.pdf"),
                "motion carried".to_string(),
            ),
        ]);
        let opts = SearchOpts {
            files: true,
            ..SearchOpts::default()
        };
        assert_eq!(
            find_file_matches(&data, &[0, 1, 2], &paths, &text, &re("SAWMILL", opts)),
            vec![(0, 1)]
        );
        assert!(find_file_matches(&data, &[1, 2], &paths, &text, &re("sawmill", opts)).is_empty());
    }

    #[test]
    fn blank_query_matches_nothing() {
        assert!(compile_search("   ", SearchOpts::default()).is_none());
    }

    #[test]
    fn search_toggles_case_word_and_regex() {
        let data = rows(&[&["Apple pie"], &["pineapple"]]);
        let all = &[0, 1][..];
        let opt = |case, word, regex| SearchOpts {
            case,
            word,
            regex,
            ..SearchOpts::default()
        };

        // Match case: "apple" no longer hits the capitalized "Apple pie".
        assert_eq!(
            find_matches(&data, all, 1, &re("apple", opt(true, false, false))),
            vec![(1, 0)]
        );
        // Whole word: "apple" hits "Apple pie" but not the substring in "pineapple".
        assert_eq!(
            find_matches(&data, all, 1, &re("apple", opt(false, true, false))),
            vec![(0, 0)]
        );
        // Regex: alternation matches both rows; an unparseable pattern matches nothing.
        assert_eq!(
            find_matches(&data, all, 1, &re("pie|pine", opt(false, false, true))),
            vec![(0, 0), (1, 0)]
        );
        assert!(compile_search("(", opt(false, false, true)).is_none());
    }

    /// Replace substitutes the match *inside* the cell — the text around it survives, and a cell
    /// containing the needle twice gets both occurrences.
    #[test]
    fn replace_rewrites_only_the_match_inside_the_cell() {
        let data = rows(&[&["Vancouver, BC"], &["BC ferries, BC coast"], &["Alberta"]]);
        let edits = replace_edits(
            &data,
            &[0, 1, 2],
            1,
            &re("BC", SearchOpts::default()),
            "British Columbia",
            SearchOpts::default(),
            None,
        );
        assert_eq!(
            edits,
            vec![
                (0, 0, "Vancouver, British Columbia".into()),
                (
                    1,
                    0,
                    "British Columbia ferries, British Columbia coast".into()
                ),
            ],
            "surrounding text survives; the non-matching row produces no edit"
        );
    }

    /// `only` is a `(view_row, data_col)` hit from the find bar, but the edit it produces must
    /// address the *source* row — the trap `filtered_view_index_diverges_from_source_index` guards.
    #[test]
    fn replace_writes_source_rows_under_a_filter() {
        let data = rows(&[&["hide"], &["show me"], &["hide"], &["show me"]]);
        let visible = compute_visible_rows(&data, &filters(&[&["hide"]]), "");
        assert_eq!(visible, vec![1, 3]);
        let opts = SearchOpts::default();
        // One hit only: view row 1, which is source row 3.
        assert_eq!(
            replace_edits(
                &data,
                &visible,
                1,
                &re("show", opts),
                "keep",
                opts,
                Some((1, 0))
            ),
            vec![(3, 0, "keep me".into())]
        );
        // Replace All never touches the filtered-out rows.
        assert_eq!(
            replace_edits(&data, &visible, 1, &re("show", opts), "keep", opts, None),
            vec![(1, 0, "keep me".into()), (3, 0, "keep me".into())]
        );
    }

    /// `$1` is a capture reference only when the user asked for regex; in literal mode a typed `$`
    /// is just a dollar sign.
    #[test]
    fn replace_expands_captures_only_in_regex_mode() {
        let data = rows(&[&["Smith, Jane"]]);
        let regex = SearchOpts {
            regex: true,
            ..SearchOpts::default()
        };
        assert_eq!(
            replace_edits(
                &data,
                &[0],
                1,
                &re(r"(\w+), (\w+)", regex),
                "$2 $1",
                regex,
                None
            ),
            vec![(0, 0, "Jane Smith".into())]
        );
        let literal = rows(&[&["cost 5"]]);
        assert_eq!(
            replace_edits(
                &literal,
                &[0],
                1,
                &re("5", SearchOpts::default()),
                "$5",
                SearchOpts::default(),
                None
            ),
            vec![(0, 0, "cost $5".into())],
            "a literal replacement must not be read as a capture group"
        );
    }

    /// The mapping every edit/selection path goes through: in a filtered view a view index and
    /// its source index diverge, so `visible_rows[view]` is the only correct row to write.
    #[test]
    fn filtered_view_index_diverges_from_source_index() {
        let g = rows(&[&["hide"], &["show"], &["hide"], &["show"]]);
        let visible = compute_visible_rows(&g, &filters(&[&["hide"]]), "");
        assert_eq!(visible, vec![1, 3]);
        assert_eq!(visible[1], 3, "view row 1 must resolve to source row 3");
    }
}

/// The delegate's own state — the shift-click range and column freezing — needs a live `App` to
/// build, unlike the free functions above. Same harness as `filter.rs`.
#[cfg(test)]
mod app_tests {
    // Never `use super::*` here — see the note on `note.rs`'s test module.
    use crate::history::{Step, moves_rows};
    use crate::{TablePanel, TableStateHandle};
    use gpui::{Entity, TestAppContext};
    use gpui_component::table::{ColumnFixed, TableDelegate as _, TableState};
    use settings::history::{Change, Origin};

    fn project() -> settings::project::CurrentProject {
        settings::project::CurrentProject {
            file: std::env::temp_dir().join("qrate-delegate-range.qrate"),
            data: settings::project::ProjectData {
                name: "T".into(),
                columns: Vec::new(),
                headers: vec!["Medium".into(), "Title".into()],
                rows: vec![
                    vec!["Film".into(), "one".into()],
                    vec!["Video".into(), "two".into()],
                    vec!["Video".into(), "three".into()],
                    vec!["Film".into(), "four".into()],
                ],
                row_ids: vec![1, 2, 3, 4],
                values: Default::default(),
            },
        }
    }

    fn table(cx: &mut TestAppContext) -> Entity<TableState<super::QrateTableDelegate>> {
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

    /// A column's key is its name, so moving one carries its identity (and its settings) to the
    /// new position, and the saved snapshot is just the display order.
    #[gpui::test]
    fn a_column_key_is_its_name_and_travels_with_it(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                assert_eq!(state.delegate().column_key(0), "Medium");

                let delegate = state.delegate_mut();
                delegate.columns.swap(0, 1);
                super::move_col(&mut delegate.rows, 0, 1);
                assert_eq!(delegate.column_key(0), "Title");

                let (headers, _, rows) = delegate.dataset_snapshot();
                assert_eq!(headers, vec!["Title", "Medium"]);
                assert_eq!(rows[0], vec!["one", "Film"]);
            });
        });
    }

    /// The range is stored in view coordinates but has to hand back source rows, so a range drawn
    /// over a filtered view must skip the rows hidden between its ends.
    #[gpui::test]
    fn a_range_over_a_filtered_view_yields_the_source_rows_the_user_sees(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                // Rows 1 and 2 drop out; the view is now source rows 0 and 3.
                delegate.set_column_kept(0, &["Film".into()]);
                delegate.range = Some(((0, 0), (1, 1)));

                let (rows, cols) = delegate.range_cells().expect("a range is drawn");
                assert_eq!(rows, vec![0, 3]);
                assert_eq!(cols, 0..=1);
                assert!(delegate.in_range(1, 1));
                assert!(!delegate.in_range(2, 0), "past the end of the range");
            });
        });
    }

    #[gpui::test]
    fn hierarchy_projection_maps_view_rows_to_sources_and_depths(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_structure(
                    &[
                        settings::project::RowStructure {
                            row_id: 1,
                            parent_id: None,
                            level_key: "series".into(),
                            sibling_order: 0,
                            source_path: None,
                            source_kind: None,
                        },
                        settings::project::RowStructure {
                            row_id: 2,
                            parent_id: Some(1),
                            level_key: "item".into(),
                            sibling_order: 0,
                            source_path: None,
                            source_kind: None,
                        },
                    ],
                    "item",
                );
                assert_eq!(delegate.visible(), &[0, 2, 3]);
                delegate.toggle_expanded(0);
                assert_eq!(delegate.visible(), &[0, 1, 2, 3]);
                assert_eq!(delegate.row_depth(1), 1);
                assert_eq!(delegate.source(1), Some(1));
            });
        });
    }

    #[gpui::test]
    fn dropped_components_append_as_one_undoable_hierarchy(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let plan = file_ingest::ImportPlan {
                    components: vec![
                        file_ingest::PlannedComponent {
                            title: "Series".into(),
                            absolute_path: "C:/archive/Series".into(),
                            source_path: "Series".into(),
                            kind: file_ingest::EntryKind::Directory,
                            parent: None,
                            level_key: "series".into(),
                        },
                        file_ingest::PlannedComponent {
                            title: "image.jpg".into(),
                            absolute_path: "C:/archive/Series/image.jpg".into(),
                            source_path: "Series/image.jpg".into(),
                            kind: file_ingest::EntryKind::File,
                            parent: Some(0),
                            level_key: "item".into(),
                        },
                    ],
                    warnings: Vec::new(),
                };
                let root = std::path::Path::new("C:/archive");
                let delegate = state.delegate_mut();
                let resolved = file_ingest::duplicates::resolve(
                    plan,
                    &delegate.existing_components(Some(0), Some(root)),
                    settings::filenames::keys,
                    file_ingest::duplicates::DuplicatePolicy::Skip,
                );
                assert_eq!(
                    delegate.append_components(&resolved, Some(1), Some(0), None, Some(root)),
                    2
                );
                assert_eq!(delegate.row_count(), 6);
                // The dropped series queues after the existing roots; its collapsed child stays hidden.
                assert_eq!(delegate.visible(), &[0, 1, 2, 3, 4]);
                let structure = delegate.row_structure();
                assert_eq!(structure[5].parent_id, Some(structure[4].row_id));
                assert_eq!(
                    structure[5].source_path.as_deref(),
                    Some("Series/image.jpg")
                );
                assert_eq!(delegate.cell(5, 1).map(AsRef::as_ref), Some("image.jpg"));
                assert!(delegate.undo().is_some());
                assert_eq!(delegate.row_count(), 4);
                assert!(delegate.redo().is_some());
                assert_eq!(delegate.row_count(), 6);
            });
        });
    }

    /// ASNT-77: a second batch over a folder qrate already holds.
    #[gpui::test]
    fn re_dropping_a_folder_adds_only_what_is_new(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let root = std::path::Path::new("C:/archive");
                let component = |name: &str, kind, parent| file_ingest::PlannedComponent {
                    title: name.rsplit('/').next().unwrap_or(name).into(),
                    absolute_path: format!("C:/archive/{name}").into(),
                    source_path: name.into(),
                    kind,
                    parent,
                    level_key: "item".into(),
                };
                let first = file_ingest::ImportPlan {
                    components: vec![
                        component("Series", file_ingest::EntryKind::Directory, None),
                        component("Series/one.jpg", file_ingest::EntryKind::File, Some(0)),
                    ],
                    warnings: Vec::new(),
                };
                let second = file_ingest::ImportPlan {
                    components: vec![
                        component("Series", file_ingest::EntryKind::Directory, None),
                        component("Series/one.jpg", file_ingest::EntryKind::File, Some(0)),
                        component("Series/two.jpg", file_ingest::EntryKind::File, Some(0)),
                    ],
                    warnings: Vec::new(),
                };
                let resolve = |existing: &[file_ingest::duplicates::ExistingComponent], plan| {
                    file_ingest::duplicates::resolve(
                        plan,
                        existing,
                        settings::filenames::keys,
                        file_ingest::duplicates::DuplicatePolicy::Skip,
                    )
                };

                let delegate = state.delegate_mut();
                let resolved = resolve(&delegate.existing_components(Some(0), Some(root)), first);
                delegate.append_components(&resolved, Some(1), Some(0), None, Some(root));
                assert_eq!(delegate.row_count(), 6);

                let resolved = resolve(&delegate.existing_components(Some(0), Some(root)), second);
                assert_eq!(resolved.duplicates(), 2);
                // Only the new photograph arrives, and it joins the series already in the table.
                assert_eq!(
                    delegate.append_components(&resolved, Some(1), Some(0), None, Some(root)),
                    1
                );
                assert_eq!(delegate.row_count(), 7);
                let structure = delegate.row_structure();
                assert_eq!(structure[6].parent_id, Some(structure[4].row_id));
                assert!(delegate.undo().is_some());
                assert_eq!(delegate.row_count(), 6);
            });
        });
    }

    #[gpui::test]
    fn inserted_rows_appear_where_they_were_asked_for(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.insert_rows(1, None);
                assert_eq!(delegate.visible(), &[0, 1, 2, 3, 4]);
                delegate.insert_rows(3, Some(2));
                assert_eq!(delegate.visible(), &[0, 1, 2, 3, 4, 5]);
            });
        });
    }

    #[gpui::test]
    fn deleting_a_middle_parent_promotes_children_into_its_place(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let component =
                    |row_id, parent_id, sibling_order| settings::project::RowStructure {
                        row_id,
                        parent_id,
                        level_key: "item".into(),
                        sibling_order,
                        source_path: None,
                        source_kind: None,
                    };
                let delegate = state.delegate_mut();
                // 1 ─┬─ 2 ── 3
                //    └─ 4
                delegate.set_structure(
                    &[
                        component(1, None, 0),
                        component(2, Some(1), 0),
                        component(3, Some(2), 0),
                        component(4, Some(1), 1),
                    ],
                    "item",
                );
                delegate.remove_rows(&[1]);
                let parent_of = |id| {
                    delegate
                        .row_structure()
                        .iter()
                        .find(|row| row.row_id == id)
                        .map(|row| (row.parent_id, row.sibling_order))
                };
                assert_eq!(parent_of(3), Some((Some(1), 0)));
                assert_eq!(parent_of(4), Some((Some(1), 1)));
            });
        });
    }

    /// Grouping is one gesture, so the new parent and the moves under it undo together.
    #[gpui::test]
    fn grouping_rows_adds_one_parent_and_undoes_as_one_step(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.group_rows(&[1, 2]).unwrap();
                assert_eq!(delegate.row_count(), 5);
                let group = delegate.row_ids()[1];
                let parent_of = |delegate: &super::QrateTableDelegate, id| {
                    delegate
                        .row_structure()
                        .iter()
                        .find(|row| row.row_id == id)
                        .and_then(|row| row.parent_id)
                };
                assert_eq!(parent_of(delegate, 2), Some(group));
                assert_eq!(parent_of(delegate, 3), Some(group));
                assert_eq!(parent_of(delegate, group), None);
                assert_eq!(delegate.row_child_count(1), 2);

                delegate.undo();
                assert_eq!(delegate.row_count(), 4);
                assert_eq!(parent_of(delegate, 2), None);
                assert_eq!(parent_of(delegate, 3), None);
            });
        });
    }

    #[gpui::test]
    fn hierarchy_edits_and_row_deletion_restore_structure_on_undo(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_structure(
                    &[
                        settings::project::RowStructure {
                            row_id: 1,
                            parent_id: None,
                            level_key: "series".into(),
                            sibling_order: 0,
                            source_path: None,
                            source_kind: None,
                        },
                        settings::project::RowStructure {
                            row_id: 2,
                            parent_id: None,
                            level_key: "item".into(),
                            sibling_order: 1,
                            source_path: None,
                            source_kind: None,
                        },
                    ],
                    "item",
                );
                delegate.indent_row(1).unwrap();
                assert_eq!(delegate.row_structure()[1].parent_id, Some(1));
                delegate.undo();
                assert_eq!(delegate.row_structure()[1].parent_id, None);

                delegate.indent_row(1).unwrap();
                delegate.remove_rows(&[0]);
                assert_eq!(delegate.row_structure()[0].parent_id, None);
                delegate.undo();
                let structure = delegate.row_structure();
                let child = structure.iter().find(|row| row.row_id == 2).unwrap();
                assert_eq!(child.parent_id, Some(1));
            });
        });
    }

    /// What makes Copy/Cut/Paste/Clear work on a whole row or column: with no rectangle drawn, the
    /// selection itself is the range. A selected column covers only the rows the filter left
    /// visible, so copying it gives back what the user can see.
    #[gpui::test]
    fn a_row_or_column_selection_is_its_own_range(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();

                delegate.selection = Some(super::Selection::Row(2));
                let (rows, cols) = delegate.range_cells().expect("a row is selected");
                assert_eq!((rows, cols), (vec![2], 0..=1), "every column of that row");

                delegate.selection = Some(super::Selection::Column(1));
                let (rows, cols) = delegate.range_cells().expect("a column is selected");
                assert_eq!((rows, cols), (vec![0, 1, 2, 3], 1..=1));

                // Under a filter it covers the visible rows only.
                delegate.set_column_kept(0, &["Film".into()]);
                delegate.selection = Some(super::Selection::Column(1));
                let (rows, _) = delegate.range_cells().expect("a column is selected");
                assert_eq!(rows, vec![0, 3]);
            });
        });
    }

    /// The point of putting the row set behind `range_cells`: Copy/Cut/Paste/Clear widen to a
    /// ⌘-clicked selection without any of them being told it exists. Rows come back in view order,
    /// not click order, so a copy pastes in the order it was read on screen.
    #[gpui::test]
    fn a_multi_selection_covers_every_column_of_each_row(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();

                delegate.select_only_row(3);
                delegate.toggle_row(1);
                let (rows, cols) = delegate.range_cells().expect("rows are selected");
                assert_eq!(
                    (rows, cols),
                    (vec![1, 3], 0..=1),
                    "view order, not click order"
                );

                // ⌘-clicking a selected row takes it back out, and the last one standing behaves
                // exactly as a plain single selection does.
                delegate.toggle_row(3);
                let (rows, _) = delegate.range_cells().expect("one row is left");
                assert_eq!(rows, vec![1]);
            });
        });
    }

    /// Source indices, so a filter narrows what an action reaches without forgetting the rest —
    /// clearing the filter brings the hidden rows back to the selection they never left.
    #[gpui::test]
    fn a_filter_hides_selected_rows_without_dropping_them(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.select_only_row(0);
                delegate.toggle_row(1);
                delegate.toggle_row(2);
                assert_eq!(delegate.selected_source_rows(), vec![0, 1, 2]);

                // Rows 1 and 2 are Video; keeping Film leaves only source rows 0 and 3 visible.
                delegate.set_column_kept(0, &["Film".into()]);
                assert_eq!(
                    delegate.selected_source_rows(),
                    vec![0],
                    "a hidden row is not something an action should reach"
                );

                delegate.set_column_kept(0, &["Film".into(), "Video".into()]);
                assert_eq!(delegate.selected_source_rows(), vec![0, 1, 2], "all back");
            });
        });
    }

    /// Shift down the `#` column takes the run between the cursor and the click, in view order —
    /// so under a filter it takes what is on screen between them, not the source rows in between.
    #[gpui::test]
    fn extending_a_row_selection_follows_the_view(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_column_kept(0, &["Film".into()]);
                // View is source rows 0 and 3; the two Video rows between them are hidden.
                delegate.select_only_row(0);
                delegate.extend_rows_to(3);
                assert_eq!(
                    delegate.selected_source_rows(),
                    vec![0, 3],
                    "the hidden rows between the ends are not swept up"
                );
            });
        });
    }

    /// Drawn bottom-right to top-left, the same rectangle.
    #[gpui::test]
    fn a_range_drawn_backwards_covers_the_same_cells(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.range = Some(((2, 1), (1, 0)));
                let (rows, cols) = delegate.range_cells().expect("a range is drawn");
                assert_eq!(rows, vec![1, 2]);
                assert_eq!(cols, 0..=1);
            });
        });
    }

    /// Changing a filter redefines every view index, so a range recorded against the old one has
    /// to go rather than silently point at different cells.
    #[gpui::test]
    fn refiltering_drops_the_range(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.range = Some(((0, 0), (2, 1)));
                delegate.set_column_kept(0, &["Film".into()]);
                assert!(delegate.range.is_none());
            });
        });
    }

    /// Every row-indexed vector has to move together, or a row's photo ends up on its neighbour.
    #[gpui::test]
    fn inserting_and_deleting_rows_keeps_the_parallel_state_aligned(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_image_paths(vec![
                    Some("0.jpg".into()),
                    Some("1.jpg".into()),
                    Some("2.jpg".into()),
                    Some("3.jpg".into()),
                ]);

                delegate.insert_rows(1, None);
                assert_eq!(delegate.rows.len(), 5);
                assert_eq!(delegate.row_ids(), &[1, 5, 2, 3, 4]);
                assert_eq!(delegate.cell(1, 0).map(|c| c.as_ref()), Some(""));
                assert_eq!(delegate.cell(2, 1).map(|c| c.as_ref()), Some("two"));
                assert_eq!(delegate.row_image(1), None, "the new row has no photo");
                assert_eq!(delegate.row_image(2), Some("1.jpg".as_ref()));

                assert!(delegate.undo().is_some_and(|c| moves_rows(&c)));
                assert_eq!(delegate.row_ids(), &[1, 2, 3, 4]);
                assert_eq!(delegate.cell(1, 1).map(|c| c.as_ref()), Some("two"));
                assert_eq!(delegate.row_image(1), Some("1.jpg".as_ref()));
            });
        });
    }

    /// Visual results come best first, skip rows the index cannot rate, and stop at the limit.
    #[gpui::test]
    fn search_rows_reorder_the_view_and_searches_still_see_every_row(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_image_paths(vec![
                    Some("0.jpg".into()),
                    None,
                    Some("2.jpg".into()),
                    Some("3.jpg".into()),
                ]);
                let score = |path: &std::path::Path| match path.to_str() {
                    Some("0.jpg") => Some(0.2),
                    Some("2.jpg") => Some(0.9),
                    _ => None,
                };
                let rows = |d: &super::QrateTableDelegate| -> Vec<usize> {
                    super::rank(&d.linked_rows(), score)
                        .into_iter()
                        .map(|(_, row)| row)
                        .collect()
                };
                assert_eq!(rows(delegate), vec![2, 0]);

                delegate.set_search_rows(Some(vec![2]));
                assert_eq!(delegate.visible(), &[2]);
                assert_eq!(
                    rows(delegate),
                    vec![2, 0],
                    "a narrowed view still ranks every row"
                );

                delegate.set_search_rows(Some(vec![3, 0]));
                assert_eq!(delegate.visible(), &[3, 0], "hits keep their order");
                delegate.remove_rows(&[1]);
                assert_eq!(
                    delegate.visible(),
                    &[0, 1, 2],
                    "a delete drops the stale hits"
                );
            });
        });
    }

    /// A multi-row delete is one undo step, and the deleted rows come back where they were rather
    /// than all at the first index.
    #[gpui::test]
    fn deleting_several_rows_undoes_as_one_step(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.remove_rows(&[2, 0]);
                assert_eq!(delegate.row_ids(), &[2, 4]);
                let names = |d: &super::QrateTableDelegate| {
                    (0..d.rows.len())
                        .map(|r| d.cell(r, 1).cloned().unwrap_or_default())
                        .collect::<Vec<_>>()
                };
                assert_eq!(names(delegate), vec!["two", "four"]);

                assert!(delegate.undo().is_some_and(|c| moves_rows(&c)));
                assert_eq!(delegate.row_ids(), &[1, 2, 3, 4]);
                assert_eq!(names(delegate), vec!["one", "two", "three", "four"]);
                assert_eq!(delegate.undo(), None, "one step, not two");
            });
        });
    }

    /// Replace All rewrites however many cells as a single `Step::Cells`, so one Ctrl+Z puts the
    /// whole sweep back. This is what the feature was deferred on originally.
    #[gpui::test]
    fn replace_all_is_one_undo_step(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                let media = |d: &super::QrateTableDelegate| {
                    (0..d.rows.len())
                        .map(|r| d.cell(r, 0).cloned().unwrap_or_default())
                        .collect::<Vec<_>>()
                };
                let opts = super::SearchOpts::default();
                let video = super::compile_search("Video", opts).expect("a plain query compiles");
                let edits = delegate.replace_edits(&video, "Videotape", opts, None);
                assert_eq!(edits.len(), 2);
                delegate.apply_edit(edits, Origin::Typed);
                assert_eq!(
                    media(delegate),
                    vec!["Film", "Videotape", "Videotape", "Film"]
                );

                assert!(delegate.undo().is_some_and(|c| !moves_rows(&c)));
                assert_eq!(media(delegate), vec!["Film", "Video", "Video", "Film"]);
                assert_eq!(delegate.undo(), None, "one step, not two");
            });
        });
    }

    /// Undo is strictly LIFO, which is what lets a cell edit recorded against row 3 replay
    /// correctly after a delete that moved row 3: the delete is already undone by then.
    #[gpui::test]
    fn an_edit_under_a_row_delete_undoes_to_the_right_cell(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.apply_edit(vec![(3, 1, "edited".into())], Origin::Typed);
                delegate.remove_rows(&[0]);
                // "four" is row 2 now, and its edit was recorded against row 3.
                assert_eq!(delegate.cell(2, 1).map(|c| c.as_ref()), Some("edited"));

                assert!(delegate.undo().is_some_and(|c| moves_rows(&c)));
                assert!(delegate.undo().is_some_and(|c| !moves_rows(&c)));
                assert_eq!(delegate.cell(3, 1).map(|c| c.as_ref()), Some("four"));
            });
        });
    }

    #[gpui::test]
    fn spreadsheet_rows_append_as_one_undoable_import(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                assert_eq!(
                    delegate.append_spreadsheet_rows(vec![
                        vec!["Photograph".into(), "five".into()],
                        vec!["Film".into(), "six".into()],
                    ]),
                    2
                );
                assert_eq!(delegate.cell(4, 1).map(|cell| cell.as_ref()), Some("five"));
                assert_eq!(delegate.cell(5, 1).map(|cell| cell.as_ref()), Some("six"));
                assert_eq!(
                    delegate.unsaved.last().map(|entry| &entry.origin),
                    Some(&Origin::Import)
                );
                assert!(delegate.undo().is_some_and(|changes| moves_rows(&changes)));
                assert_eq!(delegate.row_ids().len(), 4);
                assert_eq!(delegate.undo(), None);
            });
        });
    }

    /// The column, its cells, its position and its filter all come back — a delete that lost any
    /// of them would be data loss the moment autosave ran.
    #[gpui::test]
    fn deleting_a_column_undoes_whole(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_column_filter_enabled(0, true);

                delegate.remove_column(0);
                assert_eq!(delegate.column_count(), 1);
                assert_eq!(delegate.column_name(0), "Title");
                assert_eq!(delegate.cell(0, 0).map(|c| c.as_ref()), Some("one"));

                assert!(delegate.undo().is_some_and(|c| !moves_rows(&c)));
                assert_eq!(delegate.column_name(0), "Medium");
                assert_eq!(delegate.cell(3, 0).map(|c| c.as_ref()), Some("Film"));
                assert!(delegate.column_filter_enabled(0));
            });
        });
    }

    /// A name is an identity now, so a new column can't be handed one that is taken, and a rename
    /// onto an existing name is refused rather than quietly merging two columns' settings.
    #[gpui::test]
    fn column_names_stay_unique(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                let name = delegate.insert_column(1);
                assert_eq!(name, "Column 3");
                assert_eq!(delegate.column_key(1), "Column 3");

                assert!(!delegate.rename_column(1, "Medium".into()), "already taken");
                assert!(!delegate.rename_column(1, "  ".into()), "blank");
                assert!(delegate.rename_column(1, "Date".into()));
                assert_eq!(delegate.column_key(1), "Date");

                assert!(delegate.undo().is_some_and(|c| !moves_rows(&c)));
                assert_eq!(delegate.column_name(1), "Column 3");
            });
        });
    }

    /// A structural change redefines every view index, so the range recorded against the old one
    /// has to go — the same rule refiltering follows.
    #[gpui::test]
    fn deleting_a_row_under_a_filter_renarrows_and_drops_the_range(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.set_column_kept(0, &["Film".into()]);
                assert_eq!(delegate.visible_rows, vec![0, 3]);
                delegate.range = Some(((0, 0), (1, 1)));

                // Source row 0 is one of the two the filter left visible.
                delegate.remove_rows(&[0]);
                assert_eq!(delegate.visible_rows, vec![2], "\"four\" moved up to row 2");
                assert!(delegate.range.is_none());
            });
        });
    }

    #[gpui::test]
    fn frozen_columns_are_the_leading_ones_and_the_count_is_clamped(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let delegate = state.delegate_mut();
                delegate.set_frozen(99);
                assert_eq!(delegate.frozen(), 2, "clamped to the columns that exist");

                delegate.set_frozen(1);
                // Table column 0 is the pinned `#`, so data column 0 is table column 1.
                assert_eq!(delegate.column(1, cx).fixed, Some(ColumnFixed::Left));
                assert_eq!(delegate.column(2, cx).fixed, None);

                delegate.set_frozen(0);
                assert_eq!(delegate.column(1, cx).fixed, None);
            });
        });
    }

    /// The log is complete enough to walk back: undoing every change made after an entry, newest
    /// first and by identity, lands on exactly the grid that entry left — through deletes, renames
    /// and moves that shuffle every position the early changes were written against. The restore
    /// is one more entry, and one undo takes it back.
    #[gpui::test]
    fn restoring_to_an_entry_reproduces_the_grid_it_left(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.apply_edit(vec![(1, 1, "TWO".into())], Origin::Typed);
                let checkpoint = delegate.dataset_snapshot();
                let kept = delegate.unsaved_history().len();

                delegate.remove_rows(&[0]);
                delegate.insert_column(1);
                delegate.rename_column(0, "Format".into());
                delegate.shift_column(0, 2);
                delegate.record(Step::ColumnMoved { from: 0, to: 2 }, Origin::Structure);
                delegate.apply_edit(vec![(2, 2, "4".into())], Origin::Paste);
                delegate.insert_rows(0, Some(1));
                let head = delegate.dataset_snapshot();

                let undo: Vec<Change> = delegate.unsaved_history()[kept..]
                    .iter()
                    .rev()
                    .flat_map(|e| e.changes.iter().rev().map(Change::inverse))
                    .collect();
                let applied = delegate.restore(&undo, 7);
                assert_eq!(applied.len(), undo.len(), "every change still fits");
                assert_eq!(delegate.dataset_snapshot(), checkpoint);
                assert_eq!(
                    delegate.unsaved_history().last().map(|e| e.origin.clone()),
                    Some(Origin::Restore(7))
                );

                assert!(delegate.undo().is_some());
                assert_eq!(delegate.dataset_snapshot(), head);
            });
        });
    }

    /// Undo and redo are entries in their own right, and say what they put back.
    #[gpui::test]
    fn history_keeps_the_author_from_each_edit(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.apply_edit(vec![(1, 1, "First".into())], Origin::Typed);
                delegate.stamp_pending_author(Some("Avery".into()));
                delegate.apply_edit(vec![(1, 1, "Second".into())], Origin::Typed);
                delegate.stamp_pending_author(Some("Blair".into()));
                assert_eq!(
                    delegate.unsaved_history()[0].author.as_deref(),
                    Some("Avery")
                );
                assert_eq!(
                    delegate.unsaved_history()[1].author.as_deref(),
                    Some("Blair")
                );
            });
        });
    }

    #[gpui::test]
    fn undo_is_logged_as_the_inverse_of_what_it_reverses(cx: &mut TestAppContext) {
        let state = table(cx);
        cx.update(|cx| {
            state.update(cx, |state, _| {
                let delegate = state.delegate_mut();
                delegate.apply_edit(vec![(3, 1, "FOUR".into())], Origin::Typed);
                delegate.undo();
                let log = delegate.unsaved_history();
                assert_eq!(log.len(), 2);
                assert_eq!(log[1].origin, Origin::Undo);
                assert_eq!(
                    log[1].changes,
                    vec![Change::Cell {
                        row: 4,
                        column: "Title".into(),
                        before: "FOUR".into(),
                        after: "four".into(),
                    }]
                );
            });
        });
    }
}
