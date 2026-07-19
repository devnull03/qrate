use std::collections::{BTreeSet, HashSet};
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

use crate::{cell, editing::EditState, filter, row_index, search};

/// A row index into the *view* — the filtered, narrowed set the library asks about and reports
/// events in. Distinct from [`SourceIx`] so the two can't be confused at a call site: today the
/// grid is unfiltered so view == source, but a filtered view breaks that and every boundary
/// must convert. The confusion lives at exactly three spots (`rows_count`, `render_td`, and
/// `TablePanel`'s event bridge), so this pair plus [`QrateTableDelegate::source`] is the whole
/// index-safety story — no arithmetic traits, no generic index machinery.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ViewIx(pub usize);

/// A row index into the *source* data (`grid.rows`, `image_paths`), stable under filtering.
/// Selection and [`EditState`] store these so a cell edit lands on the right row even in a
/// filtered view, and so the Details panel / status bar index the real data with zero changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SourceIx(pub usize);

/// Emitted whenever the table's selection, a cell's text, or the column layout changes, so
/// cross-crate listeners (the status-bar cell readout, the Details panel) know to re-render.
/// `TablePanel` bridges the library's own `TableEvent`s into this single signal.
pub struct TableChanged;

impl EventEmitter<TableChanged> for TableState<QrateTableDelegate> {}

/// The table's current selection, mirrored from the library's native `TableEvent`s by
/// `TablePanel`'s event bridge. Cell/row/column are distinct variants because the status-bar
/// readout shows each differently, and a whole-column selection has no row to hang on a tuple.
/// Columns are data-relative (the pinned `#` column excluded) and 0-based — display adds 1.
/// Rows are `SourceIx` values (converted from the library's view index at the event bridge) so
/// the selection survives a filter change and cross-crate readers index the real data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Cell { row: usize, col: usize },
    Row(usize),
    Column(usize),
}

/// Row/column text storage, kept separate from `QrateTableDelegate` so it's unit-testable
/// without a live gpui `App` (the delegate itself needs one, to hold the editor `Entity`).
#[derive(Default)]
struct RowGrid {
    rows: Vec<Vec<SharedString>>,
}

impl RowGrid {
    fn rows_count(&self) -> usize {
        self.rows.len()
    }

    fn cell(&self, row: usize, col: usize) -> Option<&SharedString> {
        self.rows.get(row).and_then(|r| r.get(col))
    }

    fn set_cell(&mut self, row: usize, col: usize, value: SharedString) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = value;
        }
    }

    /// Move every row's cell at `from` to position `to`, mirroring a column move so cell
    /// lookups stay positional (display order == storage order).
    fn move_col(&mut self, from: usize, to: usize) {
        for row in &mut self.rows {
            if from < row.len() && to < row.len() {
                let cell = row.remove(from);
                row.insert(to, cell);
            }
        }
    }
}

/// Saved column layout — display order + widths — persisted into the project's `.qrate` file.
/// Keys are assigned per original data column at load time (`c{n}`), so they track a column's
/// identity across moves; a layout whose key set doesn't match the current data is ignored.
#[derive(Serialize, Deserialize)]
pub struct ColumnLayout {
    pub keys: Vec<String>,
    pub widths: Vec<f32>,
}

/// Data + column model for the center table. In `gpui_component` the delegate *is* the model:
/// the virtualized `DataTable` calls back into it for counts and per-cell rendering.
pub struct QrateTableDelegate {
    columns: Vec<Column>,
    grid: RowGrid,
    /// Last selection reported by the table's native `TableEvent`s. Written only by
    /// `TablePanel`'s event bridge; readers treat it as the current selection. Kept here (not
    /// read from `TableState` directly) because the library's `selected_cell()` goes stale in
    /// row-selection mode.
    pub(crate) selection: Option<Selection>,
    pub(crate) editing: EditState,
    /// Shared single-line editor, reused across whichever cell is being edited.
    pub(crate) editor: Entity<InputState>,
    /// Each row's resolved image path, parallel to `grid.rows` — populated by `TablePanel` via
    /// `set_image_paths` after `set_data`, since resolving requires the project's files-folder
    /// setting which this crate doesn't own. `None` until then, or for a row with no match.
    image_paths: Vec<Option<PathBuf>>,
    /// View→source row mapping: `visible_rows[view] == source`. The library only ever sees this
    /// narrowed set — `rows_count` returns its length and `render_td` maps through it — so
    /// per-column filtering composes with the virtualized render for free (only the visible
    /// range is asked about). Reset to identity by `set_data`; rebuilt by `recompute_visible`.
    visible_rows: Vec<usize>,
    /// Per-data-column filter, parallel to `columns`: the set of *excluded* (unchecked) cell
    /// values for that column, à la a Google Sheets "filter by values" list. Empty = no filter.
    /// A source row is visible iff, for every column, its cell value is not in that column's
    /// excluded set. Reordered alongside `columns` on move/layout so a filter tracks its column.
    filters: Vec<HashSet<SharedString>>,
    /// Shared "search this list" box for the open column-filter popover — narrows which distinct
    /// values the checklist shows (it does not itself filter the table). One entity, reused
    /// across columns since only one popover is open at a time.
    pub(crate) filter_search: Entity<InputState>,
}

impl QrateTableDelegate {
    pub(crate) fn new(editor: Entity<InputState>, filter_search: Entity<InputState>) -> Self {
        Self {
            columns: Vec::new(),
            grid: RowGrid::default(),
            selection: None,
            editing: EditState::Idle,
            editor,
            image_paths: Vec::new(),
            visible_rows: Vec::new(),
            filters: Vec::new(),
            filter_search,
        }
    }

    /// View→source conversion at the single library boundary. `None` when the view index is
    /// stale for the current filtered set (e.g. a late event referencing a now-hidden row).
    pub(crate) fn source(&self, view: ViewIx) -> Option<SourceIx> {
        self.visible_rows.get(view.0).copied().map(SourceIx)
    }

    /// Rebuild `visible_rows` from the active column filters.
    fn recompute_visible(&mut self) {
        self.visible_rows = compute_visible_rows(&self.grid.rows, &self.filters);
    }

    /// The distinct values in a data column, sorted — the universe the filter checklist offers.
    /// Drawn from *all* source rows (not just visible ones) so an excluded value can be
    /// re-checked to bring its rows back.
    pub(crate) fn column_values(&self, data_col: usize) -> Vec<SharedString> {
        let mut seen = BTreeSet::new();
        for row in &self.grid.rows {
            if let Some(v) = row.get(data_col) {
                seen.insert(v.clone());
            }
        }
        seen.into_iter().collect()
    }

    /// Toggle a single value in/out of a column's excluded set, then renarrow. Drops any
    /// in-flight edit, whose cell may have just been filtered out from under the editor.
    pub(crate) fn toggle_filter_value(&mut self, data_col: usize, value: &SharedString) {
        if let Some(excluded) = self.filters.get_mut(data_col)
            && !excluded.remove(value)
        {
            excluded.insert(value.clone());
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    /// Whether a value is currently excluded (unchecked) for a column.
    pub(crate) fn is_filter_excluded(&self, data_col: usize, value: &SharedString) -> bool {
        self.filters
            .get(data_col)
            .is_some_and(|e| e.contains(value))
    }

    /// Clear a column's filter entirely (re-check every value), then renarrow.
    pub(crate) fn clear_column_filter(&mut self, data_col: usize) {
        if let Some(excluded) = self.filters.get_mut(data_col) {
            if excluded.is_empty() {
                return;
            }
            excluded.clear();
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    /// Exclude *every* value in a column (uncheck all — the checklist's "Clear" affordance),
    /// then renarrow. Rows reappear as values are re-checked or the filter is cleared.
    pub(crate) fn exclude_all_in_column(&mut self, data_col: usize) {
        let all: HashSet<SharedString> = self.column_values(data_col).into_iter().collect();
        if let Some(excluded) = self.filters.get_mut(data_col) {
            *excluded = all;
        }
        self.recompute_visible();
        self.editing = EditState::Idle;
    }

    /// Whether a column currently narrows the view (has any excluded value) — drives the
    /// header's filter-icon "active" state.
    pub(crate) fn column_has_filter(&self, data_col: usize) -> bool {
        self.filters.get(data_col).is_some_and(|e| !e.is_empty())
    }

    /// Cells in the *visible* set matching `needle`, as `(view_row, data_col)` in view order —
    /// ready for `set_selected_cell`/`scroll_to_row` after the pinned `+1` on the column. Global
    /// find scans the filtered view so search and filter compose (find within what's shown).
    pub(crate) fn search_matches(&self, needle: &str) -> Vec<(usize, usize)> {
        find_matches(
            &self.grid.rows,
            &self.visible_rows,
            self.columns.len(),
            needle,
        )
    }

    /// Replaces the whole column/row model with real project data (headers →
    /// columns, one grid cell per row cell). Clears any selection/edit state,
    /// which may index into the old shape.
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
        self.grid.rows = rows
            .iter()
            .map(|r| r.iter().map(|c| SharedString::from(c.clone())).collect())
            .collect();
        self.selection = None;
        self.editing = EditState::Idle;
        // Fresh data: no filters, view is the identity over every source row.
        self.filters = vec![HashSet::new(); self.columns.len()];
        self.visible_rows = (0..self.grid.rows_count()).collect();
        // Stale — indexes into the old row set. Cleared until `TablePanel` re-resolves and
        // calls `set_image_paths`; until then rows show the Details panel's no-image fallback.
        self.image_paths = vec![None; self.grid.rows_count()];
    }

    /// Replaces the per-row resolved image paths — called by `TablePanel` right after
    /// `set_data`, once it has resolved the project's files folder (`table::photos`). A length
    /// mismatch (stale call racing a newer `set_data`) is ignored rather than panicking.
    pub fn set_image_paths(&mut self, paths: Vec<Option<PathBuf>>) {
        if paths.len() == self.grid.rows_count() {
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
        self.grid.cell(row, col)
    }

    /// The header text of a data column, for the filter dropdown's title.
    pub(crate) fn column_name(&self, data_col: usize) -> SharedString {
        self.columns
            .get(data_col)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_cell(&mut self, row: usize, col: usize, value: SharedString) {
        self.grid.set_cell(row, col, value);
    }

    /// The current selection — a cell, a whole row, or a whole column.
    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    /// The row's cells as `(column header, cell text)` pairs, in display order — what the
    /// Details panel shows. Iterating `self.columns` directly keeps the pinned row-index
    /// column's `+1` offset (see `column`) contained in this crate.
    pub fn row_fields(&self, row: usize) -> Vec<(SharedString, SharedString)> {
        self.columns
            .iter()
            .enumerate()
            .map(|(c, col)| {
                (
                    col.name.clone(),
                    self.grid.cell(row, c).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Current column layout (keys + widths in display order) for persistence.
    pub(crate) fn column_layout(&self) -> ColumnLayout {
        ColumnLayout {
            keys: self.columns.iter().map(|c| c.key.to_string()).collect(),
            widths: self.columns.iter().map(|c| c.width.as_f32()).collect(),
        }
    }

    /// Update column widths from a `ColumnWidthsChanged` event. The event's vec is over table
    /// columns, so index 0 is the pinned row-index column — skipped.
    pub(crate) fn set_column_widths(&mut self, widths: &[Pixels]) {
        for (col, w) in self.columns.iter_mut().zip(widths.iter().skip(1)) {
            col.width = *w;
        }
    }

    /// Apply a saved layout: reorder columns (and every row's cells, keeping lookups
    /// positional) and set widths. Ignored unless the saved keys are exactly a permutation of
    /// the current ones — a changed header set makes the layout stale.
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
        for row in &mut self.grid.rows {
            if row.len() == perm.len() {
                *row = perm.iter().map(|&i| row[i].clone()).collect();
            }
        }
        // A filter belongs to its column's data, so it rides along with the reorder. Row order
        // is untouched, so `visible_rows` stays valid — no recompute needed.
        if self.filters.len() == perm.len() {
            self.filters = perm.iter().map(|&i| self.filters[i].clone()).collect();
        }
    }
}

/// Source-row indices that pass every column's excluded-value filter, in order. Split out of the
/// delegate (which needs a live gpui `App` for its editor entity) so the filtering logic is
/// unit-testable on plain data. Fast path: no active filter → the identity mapping.
fn compute_visible_rows(
    rows: &[Vec<SharedString>],
    filters: &[HashSet<SharedString>],
) -> Vec<usize> {
    if filters.iter().all(HashSet::is_empty) {
        return (0..rows.len()).collect();
    }
    (0..rows.len())
        .filter(|&r| {
            filters.iter().enumerate().all(|(c, excluded)| {
                excluded.is_empty() || rows[r].get(c).is_none_or(|v| !excluded.contains(v))
            })
        })
        .collect()
}

/// Cells matching `needle` (case-insensitive substring) within the visible set, as
/// `(view_row, data_col)` in view order. `cols` bounds the data columns scanned. Split out for
/// the same unit-testability reason as [`compute_visible_rows`].
fn find_matches(
    rows: &[Vec<SharedString>],
    visible: &[usize],
    cols: usize,
    needle: &str,
) -> Vec<(usize, usize)> {
    if needle.trim().is_empty() {
        return Vec::new();
    }
    let needle = needle.to_lowercase();
    let mut hits = Vec::new();
    for (view, &source) in visible.iter().enumerate() {
        for col in 0..cols {
            if rows
                .get(source)
                .and_then(|r| r.get(col))
                .is_some_and(|c| search::cell_matches(c, &needle))
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
            row_index::column()
        } else {
            self.columns[col_ix - 1].clone()
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
        // `row_ix` is a VIEW index; map it to the source row before touching any row-indexed
        // data. A stale view index (out of range) renders empty rather than panicking.
        let Some(source) = self.source(ViewIx(row_ix)) else {
            return div().into_any_element();
        };
        if col_ix == row_index::COL_IX {
            // Show the source row number — the row's stable identity, not its shifting position
            // within a filtered view.
            row_index::render_td(source.0, cx)
        } else {
            cell::render_cell(self, source.0, col_ix - 1, window, cx)
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
        self.grid.move_col(from, to);
        // The column's filter moves with it (row order is untouched, so `visible_rows` stays
        // valid).
        if from < self.filters.len() && to < self.filters.len() {
            let f = self.filters.remove(from);
            self.filters.insert(to, f);
        }
        // An in-flight edit indexes into the old column order — drop it rather than
        // let its commit land in the wrong cell.
        self.editing = EditState::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&[&str]]) -> RowGrid {
        RowGrid {
            rows: rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| SharedString::from(c.to_string()))
                        .collect()
                })
                .collect(),
        }
    }

    #[test]
    fn move_col_shifts_every_row() {
        let mut g = grid(&[&["a", "b", "c"], &["d", "e", "f"]]);
        g.move_col(0, 2);
        assert_eq!(g.cell(0, 0).unwrap().as_ref(), "b");
        assert_eq!(g.cell(0, 2).unwrap().as_ref(), "a");
        assert_eq!(g.cell(1, 2).unwrap().as_ref(), "d");
    }

    #[test]
    fn move_col_out_of_range_is_noop() {
        let mut g = grid(&[&["a", "b"]]);
        g.move_col(0, 5);
        assert_eq!(g.cell(0, 0).unwrap().as_ref(), "a");
    }

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
    fn no_filter_is_identity_view() {
        let data = rows(&[&["a", "x"], &["b", "y"], &["c", "z"]]);
        assert_eq!(
            compute_visible_rows(&data, &filters(&[&[], &[]])),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn excluding_a_value_hides_its_rows() {
        let data = rows(&[&["a", "x"], &["b", "y"], &["a", "z"]]);
        // Exclude "a" in column 0 → only the middle row (source 1) survives.
        let visible = compute_visible_rows(&data, &filters(&[&["a"], &[]]));
        assert_eq!(visible, vec![1]);
    }

    #[test]
    fn filters_across_columns_are_anded() {
        let data = rows(&[&["a", "x"], &["b", "x"], &["b", "y"]]);
        // Exclude "a" in col 0 and "y" in col 1 → source 0 (a) and source 2 (y) hidden.
        let visible = compute_visible_rows(&data, &filters(&[&["a"], &["y"]]));
        assert_eq!(visible, vec![1]);
    }

    #[test]
    fn find_is_case_insensitive_and_scoped_to_visible() {
        let data = rows(&[
            &["Apple", "red"],
            &["banana", "yellow"],
            &["apricot", "red"],
        ]);
        // Full view: "ap" hits source 0 col 0 and source 2 col 0.
        let all = compute_visible_rows(&data, &filters(&[&[], &[]]));
        assert_eq!(find_matches(&data, &all, 2, "AP"), vec![(0, 0), (2, 0)]);
        // Hide the "apricot" row (exclude it in col 0): the search must not report it, and the
        // surviving hit is addressed by its *view* row, not its source row.
        let visible = compute_visible_rows(&data, &filters(&[&["apricot"], &[]]));
        assert_eq!(visible, vec![0, 1]);
        assert_eq!(find_matches(&data, &visible, 2, "ap"), vec![(0, 0)]);
    }

    #[test]
    fn blank_query_matches_nothing() {
        let data = rows(&[&["a"]]);
        assert!(find_matches(&data, &[0], 1, "   ").is_empty());
    }

    /// The bug this whole view/source split exists to prevent: editing a cell in a *filtered*
    /// view must write to the source row the view position maps to — never to the row that
    /// happens to share the view's numeric index.
    #[test]
    fn edit_in_filtered_view_writes_source_row() {
        let mut g = grid(&[
            &["hide", "keep-0"],
            &["show", "keep-1"],
            &["hide", "keep-2"],
            &["show", "keep-3"],
        ]);
        // Exclude "hide" in col 0 → visible = [1, 3] (sources), so view row 1 == source row 3.
        let visible = compute_visible_rows(&g.rows, &filters(&[&["hide"], &[]]));
        assert_eq!(visible, vec![1, 3]);

        let view_row = 1;
        let source = visible[view_row];
        assert_ne!(
            view_row, source,
            "test is meaningless unless view != source"
        );

        // The delegate's path: map view→source, then write at source.
        g.set_cell(source, 1, SharedString::from("edited"));

        assert_eq!(
            g.cell(3, 1).unwrap().as_ref(),
            "edited",
            "wrote the mapped source row"
        );
        assert_eq!(
            g.cell(1, 1).unwrap().as_ref(),
            "keep-1",
            "the row sharing the view index must be untouched"
        );
    }
}
