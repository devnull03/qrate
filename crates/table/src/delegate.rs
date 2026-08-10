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

use crate::history::{Cells, Edit, History};
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

/// Data + column model for the center table. In `gpui_component` the delegate *is* the model:
/// the virtualized `DataTable` calls back into it for counts and per-cell rendering.
pub struct QrateTableDelegate {
    columns: Vec<Column>,
    rows: Vec<Vec<SharedString>>,
    /// Last selection reported by the table's native `TableEvent`s, written only by
    /// `TablePanel`'s event bridge (the library's `selected_cell()` goes stale in row mode).
    pub(crate) selection: Option<Selection>,
    /// Shift-click range as `(anchor, head)`, each a `(view_row, data_col)` pair. View coordinates
    /// because every rendered cell tests against this, and a source row would cost a linear lookup
    /// per cell. `None` means the selection is just the one selected cell. Dropped wherever a
    /// filter or a column move invalidates a view index.
    pub(crate) range: Option<((usize, usize), (usize, usize))>,
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
    /// Undo/redo stack for cell edits. Fed by [`Self::apply_edit`] and nothing else.
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
            selection: None,
            range: None,
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

    /// See [`Self::values_generation`].
    pub(crate) fn values_generation(&self) -> u64 {
        self.values_generation
    }

    /// View→source conversion at the single library boundary. `None` for a view index that's
    /// stale for the current filtered set.
    pub(crate) fn source(&self, view: usize) -> Option<usize> {
        self.visible_rows.get(view).copied()
    }

    /// Source→view, the inverse of [`source`](Self::source). `None` when a filter currently hides
    /// the row — a diagnostic can point at a row the user has narrowed away.
    pub fn view_row(&self, source: usize) -> Option<usize> {
        self.visible_rows.iter().position(|&s| s == source)
    }

    /// A data-column index from its header text. Diagnostics address columns by name because
    /// names are `__columns`' PRIMARY KEY and survive a column move; the positional `c{ix}` key
    /// never leaves this crate.
    pub fn data_col(&self, header: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == header)
    }

    /// A diagnostic [`Location`] from the delegate's own coordinates — a source row, a data
    /// column, or both. The only place `DATASET_MAIN` and the positional-column→name lookup meet.
    pub(crate) fn location(&self, row: Option<usize>, data_col: Option<usize>) -> Location {
        Location {
            dataset: DATASET_MAIN.into(),
            row,
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
    /// each column's stable `c{ix}` key, and the sub-delimiter that splits a cell into values.
    /// Takes a closure rather than the settings type so this crate doesn't depend on its shape, and
    /// so the `c{ix}` convention stays owned by the delegate that mints it.
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

    /// Replaces the whole column/row model with real project data. Clears any selection/edit
    /// state, which may index into the old shape.
    pub fn set_data(&mut self, headers: &[String], rows: &[Vec<String>]) {
        self.columns = headers
            .iter()
            .enumerate()
            .map(|(ix, h)| {
                Column::new(format!("c{ix}"), h.clone())
                    .width(px(120.))
                    .resizable(true)
                    .movable(true)
            })
            .collect();
        self.rows = rows
            .iter()
            .map(|r| r.iter().map(|c| SharedString::from(c.clone())).collect())
            .collect();
        self.selection = None;
        self.range = None;
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

    /// A data column's stable `c{ix}` settings key — the only route out for a private field, and
    /// the one place outside [`Self::apply_column_settings`] that the convention leaks.
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
    /// Falls back to the single selected cell when no range is drawn.
    pub(crate) fn range_cells(&self) -> Option<(Vec<usize>, RangeInclusive<usize>)> {
        let Some(((ar, ac), (hr, hc))) = self.range else {
            let Selection::Cell { row, col } = self.selection? else {
                return None;
            };
            return Some((vec![row], col..=col));
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
        self.history.push(Edit(edit));
    }

    /// Restore the cells the last edit changed. `false` when there was nothing to undo, which is
    /// what tells the caller to skip the dirty-mark and the revalidate.
    pub(crate) fn undo(&mut self) -> bool {
        let cells = self.history.undo();
        self.replay(cells)
    }

    /// [`undo`](Self::undo)'s mirror.
    pub(crate) fn redo(&mut self) -> bool {
        let cells = self.history.redo();
        self.replay(cells)
    }

    /// Apply one side of a recorded edit without re-recording it — going through `apply_edit` here
    /// would make undo its own undoable action and the stack would never drain.
    fn replay(&mut self, cells: Option<Cells>) -> bool {
        let Some(cells) = cells else {
            return false;
        };
        for (row, col, text) in cells {
            self.set_cell(row, col, text);
        }
        // An open edit would commit over what was just restored.
        self.editing = EditState::Idle;
        true
    }

    /// The current selection — a cell, a whole row, or a whole column.
    pub fn selection(&self) -> Option<Selection> {
        self.selection
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

    /// Headers + rows in the project's *original* column order, recovered from each column's
    /// stable `c{ix}` key rather than the current display order. `dataset_main` keeps a fixed
    /// physical column order (the separately-saved [`ColumnLayout`] maps it to display order), so
    /// a save after a column move must not reshuffle the stored columns. Cells are stringified for
    /// the `.qrate` writer, and for every export.
    pub fn dataset_snapshot(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let mut order: Vec<usize> = (0..self.columns.len()).collect();
        order.sort_by_key(|&i| orig_col_ix(&self.columns[i]));
        let headers = order
            .iter()
            .map(|&i| self.columns[i].name.to_string())
            .collect();
        let rows = self
            .rows
            .iter()
            .map(|row| {
                order
                    .iter()
                    .map(|&i| row.get(i).map(|c| c.to_string()).unwrap_or_default())
                    .collect()
            })
            .collect();
        (headers, rows)
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

/// The original column index encoded in a `c{ix}` key (minted in [`QrateTableDelegate::set_data`]).
/// An unexpected key sorts last (`usize::MAX`) rather than colliding at 0.
fn orig_col_ix(col: &Column) -> usize {
    col.key
        .as_ref()
        .strip_prefix('c')
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX)
}

/// Move every row's cell at `from` to position `to`, mirroring a column move so cell lookups
/// stay positional (display order == storage order).
/// An inclusive range between two endpoints given in either order — a drag up-and-left selects the
/// same rectangle as the same drag down-and-right.
fn normalize(a: usize, b: usize) -> RangeInclusive<usize> {
    a.min(b)..=a.max(b)
}

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
            let highlighted = self.active_cell().is_some_and(|(r, _)| r == source);
            row_index::render_td(self, source, highlighted, cx)
        } else {
            let ranged = self.in_range(row_ix, col_ix - 1);
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
    fn snapshot_order_follows_ckey_not_display_order() {
        // A display where columns sit out of their original `c{ix}` order (as after moves).
        let cols = [
            Column::new("c2", "C"),
            Column::new("c0", "A"),
            Column::new("c1", "B"),
        ];
        let mut order: Vec<usize> = (0..cols.len()).collect();
        order.sort_by_key(|&i| orig_col_ix(&cols[i]));
        let names: Vec<&str> = order.iter().map(|&i| cols[i].name.as_ref()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
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
