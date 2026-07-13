use gpui::{App, Context, Entity, EventEmitter, IntoElement, SharedString, Window, px};
use gpui_component::{
    input::InputState,
    table::{Column, TableDelegate, TableState},
};

use crate::{editing::EditState, row_index, selection};

const COLUMN_COUNT: usize = 20;

/// Emitted whenever the selected cell or a cell's text changes, so cross-crate listeners (the
/// status-bar cell readout) know to re-render. `gpui_component`'s own `TableEvent` only fires
/// for its independent row/column selection, which this crate doesn't use — see `selection.rs`.
pub struct TableChanged;

impl EventEmitter<TableChanged> for TableState<QrateTableDelegate> {}

/// Row/column text storage, kept separate from `QrateTableDelegate` so it's unit-testable
/// without a live gpui `App` (the delegate itself needs one, to hold the editor `Entity`).
#[derive(Default)]
struct RowGrid {
    rows: Vec<Vec<SharedString>>,
}

impl RowGrid {
    /// Append `n` rows of placeholder text, each with `columns` cells. Every cell is
    /// `"cell {r}-{c} lorem"` — deliberately three whitespace-separated words so the status-bar
    /// word-count widget has a known value.
    fn fill_fake(&mut self, n: usize, columns: usize) {
        let start = self.rows.len();
        self.rows.extend((0..n).map(|i| {
            let r = start + i;
            (0..columns)
                .map(|c| SharedString::from(format!("cell {r}-{c} lorem")))
                .collect()
        }));
    }

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
}

/// Data + column model for the center table. In `gpui_component` the delegate *is* the model:
/// the virtualized `Table` calls back into it for counts and per-cell rendering.
pub struct QrateTableDelegate {
    columns: Vec<Column>,
    grid: RowGrid,
    /// The single selected cell, `(row, data_col)` — this crate's own selection model, since
    /// the published `gpui_component` only supports independent row/column axes, not a combined
    /// single-cell click (see `selection.rs`). `data_col` is 0-based over `columns`, not shifted
    /// for the pinned row-index column at table column 0.
    pub(crate) selected: Option<(usize, usize)>,
    pub(crate) editing: EditState,
    /// Shared single-line editor, reused across whichever cell is being edited.
    pub(crate) editor: Entity<InputState>,
}

impl QrateTableDelegate {
    pub(crate) fn new(editor: Entity<InputState>) -> Self {
        let columns = (0..COLUMN_COUNT)
            .map(|c| Column::new(format!("c{c}"), format!("Col {c}")).width(px(120.)))
            .collect();
        Self {
            columns,
            grid: RowGrid::default(),
            selected: None,
            editing: EditState::Idle,
            editor,
        }
    }

    /// Append `n` rows of placeholder text.
    pub fn fill_fake(&mut self, n: usize) {
        self.grid.fill_fake(n, self.columns.len());
    }

    /// Replaces the whole column/row model with real project data (headers →
    /// columns, one grid cell per row cell). Clears any selection/edit state,
    /// which may index into the old shape.
    pub fn set_data(&mut self, headers: &[String], rows: &[Vec<String>]) {
        self.columns = headers
            .iter()
            .enumerate()
            .map(|(ix, h)| Column::new(format!("c{ix}"), h.clone()).width(px(120.)))
            .collect();
        self.grid.rows = rows
            .iter()
            .map(|r| r.iter().map(|c| SharedString::from(c.clone())).collect())
            .collect();
        self.selected = None;
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

    /// The single selected cell, if any — `(row, data_col)`.
    pub fn selected(&self) -> Option<(usize, usize)> {
        self.selected
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

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        if col_ix == row_index::COL_IX {
            row_index::column()
        } else {
            &self.columns[col_ix - 1]
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
            selection::render_cell(self, row_ix, col_ix - 1, window, cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RowGrid;

    const COLUMN_COUNT: usize = 20;

    #[test]
    fn fill_fake_shape_and_word_count() {
        let mut grid = RowGrid::default();
        grid.fill_fake(3, COLUMN_COUNT);
        assert_eq!(grid.rows_count(), 3);
        assert!(
            (0..3)
                .flat_map(|r| (0..COLUMN_COUNT).map(move |c| (r, c)))
                .all(|(r, c)| grid.cell(r, c).unwrap().split_whitespace().count() == 3)
        );
    }

    #[test]
    fn fill_fake_appends_without_overwriting() {
        let mut grid = RowGrid::default();
        grid.fill_fake(2, COLUMN_COUNT);
        grid.fill_fake(2, COLUMN_COUNT);
        assert_eq!(grid.rows_count(), 4);
        // Second batch's row numbers continue from the first, not restart at 0.
        assert!(grid.cell(2, 0).unwrap().starts_with("cell 2-0"));
        assert!(grid.cell(3, 0).unwrap().starts_with("cell 3-0"));
    }
}
