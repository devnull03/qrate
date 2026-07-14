use gpui::{App, Context, Entity, EventEmitter, IntoElement, Pixels, SharedString, Window, px};
use gpui_component::{
    input::InputState,
    table::{Column, TableDelegate, TableState},
};
use serde::{Deserialize, Serialize};

use crate::{cell, editing::EditState, row_index};

/// Emitted whenever the table's selection, a cell's text, or the column layout changes, so
/// cross-crate listeners (the status-bar cell readout, the Details panel) know to re-render.
/// `TablePanel` bridges the library's own `TableEvent`s into this single signal.
pub struct TableChanged;

impl EventEmitter<TableChanged> for TableState<QrateTableDelegate> {}

/// The table's current selection, mirrored from the library's native `TableEvent`s by
/// `TablePanel`'s event bridge. Cell/row/column are distinct variants because the status-bar
/// readout shows each differently, and a whole-column selection has no row to hang on a tuple.
/// Indices are data-relative (the pinned `#` column excluded) and 0-based — display adds 1.
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
}

impl QrateTableDelegate {
    pub(crate) fn new(editor: Entity<InputState>) -> Self {
        Self {
            columns: Vec::new(),
            grid: RowGrid::default(),
            selection: None,
            editing: EditState::Idle,
            editor,
        }
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
    }

    /// Cell text at `(row, col)`, if in range. `col` is a data-column index, not shifted for the
    /// pinned row-index column.
    pub fn cell(&self, row: usize, col: usize) -> Option<&SharedString> {
        self.grid.cell(row, col)
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
    }
}

impl TableDelegate for QrateTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        // +1 for the pinned row-index column at table column 0.
        self.columns.len() + 1
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.grid.rows_count()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if col_ix == row_index::COL_IX {
            row_index::column()
        } else {
            self.columns[col_ix - 1].clone()
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix == row_index::COL_IX {
            row_index::render_td(row_ix, cx)
        } else {
            cell::render_cell(self, row_ix, col_ix - 1, window, cx)
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
}
