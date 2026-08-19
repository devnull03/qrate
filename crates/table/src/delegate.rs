use std::collections::{BTreeSet, HashSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Pixels, SharedString,
    Window, div, px,
};
use gpui_component::{
    input::InputState,
    table::{Column, TableDelegate, TableState},
};
use serde::{Deserialize, Serialize};

use diagnostics::{DATASET_MAIN, Location};

use crate::history::{Cells, Col, History, Row, Step};
use crate::{cell, editing::EditState, filter, row_index};

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
    /// Shared single-line editor, reused across whichever cell is being edited.
    pub(crate) editor: Entity<InputState>,
    /// Which cell/row/column the note editor is open on, if any.
    pub(crate) note_edit: Option<Location>,
    /// Shared note editor, the same one-per-table arrangement as [`Self::editor`].
    pub(crate) note_editor: Entity<InputState>,
    /// Each row's resolved image path, parallel to `rows`. `None` until `TablePanel` resolves it.
    image_paths: Vec<Option<PathBuf>>,
    /// View→source row mapping: `visible_rows[view] == source`. The library only ever sees this
    /// narrowed set, so filtering composes with the virtualized render for free.
    visible_rows: Vec<usize>,
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
    /// How many leading data columns are frozen. A count in *display* order, not a set of keys:
    /// the library's fixed region is always the leading columns, and moving a column in or out of
    /// it is how a sheet re-freezes. Zero means only the pinned `#` column stays put.
    frozen: usize,
}

impl QrateTableDelegate {
    pub(crate) fn new(editor: Entity<InputState>, note_editor: Entity<InputState>) -> Self {
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
            visible_rows: Vec::new(),
            filters: Vec::new(),
            filters_enabled: Vec::new(),
            subdelimiter: SharedString::default(),
            values_generation: 0,
            history: History::default(),
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
    pub(crate) fn values_generation(&self) -> u64 {
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

    /// Source→view, the inverse of [`source`](Self::source). `None` when a filter currently hides
    /// the row — a diagnostic can point at a row the user has narrowed away.
    pub fn view_row(&self, source: usize) -> Option<usize> {
        self.visible_rows.iter().position(|&s| s == source)
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
        self.visible_rows = compute_visible_rows(&self.rows, &self.filters, &self.subdelimiter);
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

    /// Cells in the *visible* set matching `needle` under `opts`, as `(view_row, data_col)` in view
    /// order — ready for `set_selected_cell`/`scroll_to_row` after the pinned `+1` on the column.
    pub(crate) fn search_matches(&self, needle: &str, opts: SearchOpts) -> Vec<(usize, usize)> {
        find_matches(
            &self.rows,
            &self.visible_rows,
            self.columns.len(),
            needle,
            opts,
        )
    }

    /// The cell writes replacing `needle` with `replacement` produces, in *source* coordinates and
    /// ready for [`apply_edit`](Self::apply_edit). `only` limits it to one `(view_row, data_col)`
    /// hit — Replace vs Replace All.
    pub(crate) fn replace_edits(
        &self,
        needle: &str,
        replacement: &str,
        opts: SearchOpts,
        only: Option<(usize, usize)>,
    ) -> Cells {
        replace_edits(
            &self.rows,
            &self.visible_rows,
            self.columns.len(),
            needle,
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
        self.columns = headers
            .iter()
            .map(|h| {
                Column::new(h.clone(), h.clone())
                    .width(px(120.))
                    .resizable(true)
                    .movable(true)
            })
            .collect();
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
        self.filters = vec![HashSet::new(); self.columns.len()];
        self.filters_enabled = vec![false; self.columns.len()];
        self.visible_rows = (0..self.rows.len()).collect();
        // Stale — indexes into the old row set; `TablePanel` re-resolves right after.
        self.image_paths = vec![None; self.rows.len()];
        self.values_generation += 1;
    }

    /// Replaces the per-row resolved image paths. A length mismatch (a stale call racing a newer
    /// `set_data`) is ignored rather than panicking.
    pub fn set_image_paths(&mut self, paths: Vec<Option<PathBuf>>) {
        if paths.len() == self.rows.len() {
            self.image_paths = paths;
        }
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
        diagnostics::Validators::run(&columns, &self.rows, cx);
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
    pub(crate) fn apply_edit(&mut self, cells: Cells) {
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
        self.history.push(Step::Cells(edit));
    }

    /// Reverse the last recorded step. The answer says whether it moved rows; `None` means there
    /// was nothing to undo.
    pub(crate) fn undo(&mut self) -> Option<bool> {
        let Some(step) = self.history.undo() else {
            return None;
        };
        let rows_changed = matches!(step, Step::RowsAdded { .. } | Step::RowsRemoved(_));
        self.replay(&step, false);
        Some(rows_changed)
    }

    /// [`undo`](Self::undo)'s mirror.
    pub(crate) fn redo(&mut self) -> Option<bool> {
        let Some(step) = self.history.redo() else {
            return None;
        };
        let rows_changed = matches!(step, Step::RowsAdded { .. } | Step::RowsRemoved(_));
        self.replay(&step, true);
        Some(rows_changed)
    }

    /// Apply one side of a recorded step without re-recording it — going through `apply_edit` here
    /// would make undo its own undoable action and the stack would never drain.
    fn replay(&mut self, step: &Step, forward: bool) {
        match step {
            Step::Cells(cells) => {
                for (row, col, before, after) in cells {
                    let text = if forward { after } else { before };
                    self.set_cell(*row, *col, text.clone());
                }
            }
            Step::RowsAdded { at, rows } => {
                if forward {
                    self.splice_rows(*at, rows);
                } else {
                    self.cut_rows(&(*at..at + rows.len()).collect::<Vec<_>>());
                }
            }
            Step::RowsRemoved(rows) => {
                if forward {
                    self.cut_rows(&rows.iter().map(|(at, _)| *at).collect::<Vec<_>>());
                } else {
                    for (at, row) in rows {
                        self.splice_rows(*at, std::slice::from_ref(row));
                    }
                }
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
        }
        // An open edit would commit over what was just restored.
        self.editing = EditState::Idle;
    }

    /// Put `rows` in at `at`, keeping everything row-indexed parallel. The unrecorded half of a
    /// row insert — [`Self::insert_rows`] is the one that reaches the undo stack.
    fn splice_rows(&mut self, at: usize, rows: &[Row]) {
        let at = at.min(self.rows.len());
        for (offset, row) in rows.iter().enumerate() {
            self.rows.insert(at + offset, row.cells.clone());
            self.row_ids.insert(at + offset, row.id);
            self.image_paths.insert(at + offset, row.image.clone());
        }
        self.rows_changed();
    }

    /// Take out the rows at `ats`, in any order, and hand them back ascending — the order they go
    /// back in. [`splice_rows`](Self::splice_rows)' inverse.
    fn cut_rows(&mut self, ats: &[usize]) -> Vec<(usize, Row)> {
        let mut ats: Vec<usize> = ats
            .iter()
            .copied()
            .filter(|&r| r < self.rows.len())
            .collect();
        ats.sort_unstable();
        ats.dedup();
        let mut cut: Vec<(usize, Row)> = ats
            .iter()
            .rev()
            .map(|&at| {
                (
                    at,
                    Row {
                        id: self.row_ids.remove(at),
                        cells: self.rows.remove(at),
                        image: self.image_paths.remove(at),
                    },
                )
            })
            .collect();
        cut.reverse();
        self.rows_changed();
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

    /// What every row insert or delete invalidates. The view is rebuilt rather than patched: a
    /// filtered view's indices all move, and `recompute_visible` already drops the range for that.
    fn rows_changed(&mut self) {
        self.recompute_visible();
        self.editing = EditState::Idle;
        self.values_generation += 1;
        self.clamp_selection();
    }

    /// [`rows_changed`](Self::rows_changed)'s column counterpart. Still renarrows: a column that
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
        let cells = match source.and_then(|r| self.rows.get(r)) {
            Some(row) => row.clone(),
            None => vec![SharedString::default(); self.columns.len()],
        };
        let image = source.and_then(|r| self.image_paths.get(r).cloned().flatten());
        let id = self.fresh_row_id();
        let rows = vec![Row { id, cells, image }];
        self.splice_rows(at, &rows);
        self.history.push(Step::RowsAdded { at, rows });
    }

    /// Delete the rows at `ats` as one undo step, carrying their cells and photos on it.
    pub(crate) fn remove_rows(&mut self, ats: &[usize]) {
        let removed = self.cut_rows(ats);
        self.history.push(Step::RowsRemoved(removed));
    }

    pub(crate) fn row_id(&self, source: usize) -> Option<settings::project::RowId> {
        self.row_ids.get(source).copied()
    }

    pub(crate) fn row_ids(&self) -> &[settings::project::RowId] {
        &self.row_ids
    }

    fn fresh_row_id(&mut self) -> settings::project::RowId {
        while self.row_ids.contains(&self.next_row_id) {
            self.next_row_id = self.next_row_id.checked_add(1).unwrap_or(1);
        }
        let id = self.next_row_id;
        self.next_row_id = id.checked_add(1).unwrap_or(1);
        id
    }

    /// Add a blank column at `at`, named for the user to rename. Returns its name.
    pub(crate) fn insert_column(&mut self, at: usize) -> SharedString {
        let name = self.fresh_column_name();
        let col = Col {
            column: Column::new(name.clone(), name.clone())
                .width(px(120.))
                .resizable(true)
                .movable(true),
            cells: vec![SharedString::default(); self.rows.len()],
            excluded: HashSet::new(),
            filter_enabled: false,
        };
        self.splice_column(at, &col);
        self.history.push(Step::ColumnAdded { at, col });
        name
    }

    /// Delete a column and every row's cell in it, as one undo step.
    pub(crate) fn remove_column(&mut self, at: usize) {
        let Some(col) = self.cut_column(at) else {
            return;
        };
        self.history.push(Step::ColumnRemoved { at, col });
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
        self.history.push(Step::Renamed {
            col,
            before,
            after: name,
        });
        true
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
    needle: &str,
    opts: SearchOpts,
) -> Vec<(usize, usize)> {
    let Some(re) = compile_search(needle, opts) else {
        return Vec::new();
    };
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

/// Rewrite matches in place: `Regex::replace_all` substitutes only the matched spans, so the rest
/// of the cell's text survives and a cell with two occurrences gets both. Literal mode wraps the
/// replacement in `NoExpand` so a `$` the user typed stays a `$`; regex mode leaves `$1` capture
/// references working, which is the point of turning regex on.
fn replace_edits(
    rows: &[Vec<SharedString>],
    visible: &[usize],
    cols: usize,
    needle: &str,
    replacement: &str,
    opts: SearchOpts,
    only: Option<(usize, usize)>,
) -> Cells {
    let Some(re) = compile_search(needle, opts) else {
        return Cells::new();
    };
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
            row_index::render_td(self, source, highlighted, cx)
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
        let col = self.columns.remove(from);
        self.columns.insert(to, col);
        move_col(&mut self.rows, from, to);
        self.values_generation += 1;
        // The column's filter (and whether it has one) moves with it; row order is untouched.
        if from < self.filters.len() && to < self.filters.len() {
            let f = self.filters.remove(from);
            self.filters.insert(to, f);
        }
        if from < self.filters_enabled.len() && to < self.filters_enabled.len() {
            let e = self.filters_enabled.remove(from);
            self.filters_enabled.insert(to, e);
        }
        // An in-flight edit indexes into the old column order — drop it, and the recorded ones too.
        self.editing = EditState::Idle;
        self.range = None;
        self.history = History::default();
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
            find_matches(&data, &all, 2, "AP", opts),
            vec![(0, 0), (2, 0)]
        );
        // Hide the "apricot" row: the search must not report it, and the surviving hit is
        // addressed by its *view* row, not its source row.
        let visible = compute_visible_rows(&data, &filters(&[&["apricot"], &[]]), "");
        assert_eq!(visible, vec![0, 1]);
        assert_eq!(find_matches(&data, &visible, 2, "ap", opts), vec![(0, 0)]);
    }

    #[test]
    fn blank_query_matches_nothing() {
        let data = rows(&[&["a"]]);
        assert!(find_matches(&data, &[0], 1, "   ", SearchOpts::default()).is_empty());
    }

    #[test]
    fn search_toggles_case_word_and_regex() {
        let data = rows(&[&["Apple pie"], &["pineapple"]]);
        let all = &[0, 1][..];
        let opt = |case, word, regex| SearchOpts { case, word, regex };

        // Match case: "apple" no longer hits the capitalized "Apple pie".
        assert_eq!(
            find_matches(&data, all, 1, "apple", opt(true, false, false)),
            vec![(1, 0)]
        );
        // Whole word: "apple" hits "Apple pie" but not the substring in "pineapple".
        assert_eq!(
            find_matches(&data, all, 1, "apple", opt(false, true, false)),
            vec![(0, 0)]
        );
        // Regex: alternation matches both rows; an unparseable pattern matches nothing.
        assert_eq!(
            find_matches(&data, all, 1, "pie|pine", opt(false, false, true)),
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
            "BC",
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
            replace_edits(&data, &visible, 1, "show", "keep", opts, Some((1, 0))),
            vec![(3, 0, "keep me".into())]
        );
        // Replace All never touches the filtered-out rows.
        assert_eq!(
            replace_edits(&data, &visible, 1, "show", "keep", opts, None),
            vec![(1, 0, "keep me".into()), (3, 0, "keep me".into())]
        );
    }

    /// `$1` is a capture reference only when the user asked for regex; in literal mode a typed `$`
    /// is just a dollar sign.
    #[test]
    fn replace_expands_captures_only_in_regex_mode() {
        let data = rows(&[&["Smith, Jane"]]);
        let regex = SearchOpts {
            case: false,
            word: false,
            regex: true,
        };
        assert_eq!(
            replace_edits(&data, &[0], 1, r"(\w+), (\w+)", "$2 $1", regex, None),
            vec![(0, 0, "Jane Smith".into())]
        );
        let literal = rows(&[&["cost 5"]]);
        assert_eq!(
            replace_edits(&literal, &[0], 1, "5", "$5", SearchOpts::default(), None),
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
    use crate::{TablePanel, TableStateHandle};
    use gpui::{Entity, TestAppContext};
    use gpui_component::table::{ColumnFixed, TableDelegate as _, TableState};

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

                assert_eq!(delegate.undo(), Some(true));
                assert_eq!(delegate.row_ids(), &[1, 2, 3, 4]);
                assert_eq!(delegate.cell(1, 1).map(|c| c.as_ref()), Some("two"));
                assert_eq!(delegate.row_image(1), Some("1.jpg".as_ref()));
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

                assert_eq!(delegate.undo(), Some(true));
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
                let edits = delegate.replace_edits(
                    "Video",
                    "Videotape",
                    super::SearchOpts::default(),
                    None,
                );
                assert_eq!(edits.len(), 2);
                delegate.apply_edit(edits);
                assert_eq!(
                    media(delegate),
                    vec!["Film", "Videotape", "Videotape", "Film"]
                );

                assert_eq!(delegate.undo(), Some(false));
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
                delegate.apply_edit(vec![(3, 1, "edited".into())]);
                delegate.remove_rows(&[0]);
                // "four" is row 2 now, and its edit was recorded against row 3.
                assert_eq!(delegate.cell(2, 1).map(|c| c.as_ref()), Some("edited"));

                assert_eq!(delegate.undo(), Some(true));
                assert_eq!(delegate.undo(), Some(false));
                assert_eq!(delegate.cell(3, 1).map(|c| c.as_ref()), Some("four"));
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

                assert_eq!(delegate.undo(), Some(false));
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

                assert_eq!(delegate.undo(), Some(false));
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
}
