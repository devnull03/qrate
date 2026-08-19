//! Undo/redo for grid edits and for the structural changes that add and remove rows and columns.
//! Deliberately not a command trait: every editing path in the crate funnels through
//! `QrateTableDelegate::apply_edit` or one of its structural siblings (`insert_rows`,
//! `remove_column`, …), which record what they touched here, so a new editing feature gets undo by
//! calling one of those rather than by implementing anything.
//!
//! A [`Step`] holds enough to be replayed in *either* direction, and nothing here re-maps an index
//! after a structural change: undo is strictly LIFO, so by the time an older step replays, every
//! change made after it has already been reversed and its indices mean again what they meant.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::SharedString;
use gpui_component::table::Column;

/// A batch of cell writes as `(row, col, text)` — what `apply_edit` takes and what undo hands back.
pub(crate) type Cells = Vec<(usize, usize, SharedString)>;

/// How many steps to keep. Past this the oldest is dropped — a grid edit holds two strings, so the
/// cap is about bounding a pathological paste loop, not about memory pressure in normal use.
const CAP: usize = 200;

/// A whole row: its cells and its resolved image path, so restoring a deleted row doesn't need a
/// re-walk of the files folder to get its photo back.
#[derive(Clone)]
pub(crate) struct Row {
    pub id: settings::project::RowId,
    pub cells: Vec<SharedString>,
    pub image: Option<PathBuf>,
}

/// A whole column: its header, every row's cell for it, and the filter it was narrowing by — the
/// same reason [`Row`] carries its photo. Undo has to give a column back as it was, not as a fresh
/// one that happens to hold the same text.
#[derive(Clone)]
pub(crate) struct Col {
    pub column: Column,
    pub cells: Vec<SharedString>,
    pub excluded: HashSet<SharedString>,
    pub filter_enabled: bool,
}

/// One undoable change. A paste, a bulk fill, or a multi-row delete is a single `Step`, so it
/// undoes in one go. The `Added`/`Removed` pairs are each other's inverse, which is the whole of
/// how undo and redo are implemented.
///
/// `Clone` so a step can be handed to the delegate to replay while staying on the stack — the
/// contents are `SharedString`s and a `Column`, so a clone is refcount bumps, not a copy of the
/// column's text.
#[derive(Clone)]
pub(crate) enum Step {
    /// Every cell touched, as `(row, col, before, after)`.
    Cells(Vec<(usize, usize, SharedString, SharedString)>),
    RowsAdded {
        at: usize,
        rows: Vec<Row>,
    },
    /// Ascending by index, which is the order they have to go back in.
    RowsRemoved(Vec<(usize, Row)>),
    ColumnAdded {
        at: usize,
        col: Col,
    },
    ColumnRemoved {
        at: usize,
        col: Col,
    },
    Renamed {
        col: usize,
        before: SharedString,
        after: SharedString,
    },
}

impl Step {
    /// Whether this step changed nothing and should not consume an undo.
    fn is_empty(&self) -> bool {
        match self {
            Step::Cells(cells) => cells.is_empty(),
            Step::RowsAdded { rows, .. } => rows.is_empty(),
            Step::RowsRemoved(rows) => rows.is_empty(),
            Step::Renamed { before, after, .. } => before == after,
            _ => false,
        }
    }
}

#[derive(Default)]
pub(crate) struct History {
    done: Vec<Step>,
    undone: Vec<Step>,
}

impl History {
    /// Record a change. Drops the redo tail: a new edit forks the timeline.
    pub(crate) fn push(&mut self, step: Step) {
        if step.is_empty() {
            return;
        }
        self.undone.clear();
        self.done.push(step);
        if self.done.len() > CAP {
            self.done.remove(0);
        }
    }

    /// The last step, to be replayed backwards. `None` when there is nothing left to undo.
    pub(crate) fn undo(&mut self) -> Option<Step> {
        let step = self.done.pop()?;
        self.undone.push(step.clone());
        Some(step)
    }

    /// [`undo`](Self::undo)'s mirror: the last undone step, to be replayed forwards.
    pub(crate) fn redo(&mut self) -> Option<Step> {
        let step = self.undone.pop()?;
        self.done.push(step.clone());
        Some(step)
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Row, Step};
    use gpui::SharedString;

    fn edit(cells: &[(usize, usize, &str, &str)]) -> Step {
        Step::Cells(
            cells
                .iter()
                .map(|(r, c, b, a)| (*r, *c, SharedString::from(b.to_string()), (*a).into()))
                .collect(),
        )
    }

    /// The cells a `Step::Cells` holds, as `(row, col, before)` — what the delegate writes to undo.
    fn befores(step: &Step) -> Vec<(usize, usize, SharedString)> {
        let Step::Cells(cells) = step else {
            panic!("not a cell edit");
        };
        cells
            .iter()
            .map(|(r, c, before, _)| (*r, *c, before.clone()))
            .collect()
    }

    fn afters(step: &Step) -> Vec<(usize, usize, SharedString)> {
        let Step::Cells(cells) = step else {
            panic!("not a cell edit");
        };
        cells
            .iter()
            .map(|(r, c, _, after)| (*r, *c, after.clone()))
            .collect()
    }

    #[test]
    fn edits_undo_in_reverse_order_and_redo_forward() {
        let mut h = History::default();
        h.push(edit(&[(0, 0, "a", "A")]));
        h.push(edit(&[(1, 0, "b", "B")]));
        h.push(edit(&[(2, 0, "c", "C")]));

        assert_eq!(befores(&h.undo().unwrap()), vec![(2, 0, "c".into())]);
        assert_eq!(befores(&h.undo().unwrap()), vec![(1, 0, "b".into())]);
        assert_eq!(afters(&h.redo().unwrap()), vec![(1, 0, "B".into())]);
        assert_eq!(afters(&h.redo().unwrap()), vec![(2, 0, "C".into())]);
        assert!(h.redo().is_none());
    }

    #[test]
    fn a_new_edit_after_an_undo_drops_the_redo_tail() {
        let mut h = History::default();
        h.push(edit(&[(0, 0, "a", "A")]));
        h.undo().unwrap();
        h.push(edit(&[(9, 9, "x", "X")]));
        assert!(h.redo().is_none());
        assert_eq!(befores(&h.undo().unwrap()), vec![(9, 9, "x".into())]);
    }

    #[test]
    fn a_multi_cell_edit_undoes_as_one_step() {
        let mut h = History::default();
        h.push(edit(&[
            (0, 0, "a", "A"),
            (0, 1, "b", "B"),
            (1, 0, "c", "C"),
        ]));
        assert_eq!(
            befores(&h.undo().unwrap()),
            vec![(0, 0, "a".into()), (0, 1, "b".into()), (1, 0, "c".into())]
        );
        assert!(h.undo().is_none());
    }

    #[test]
    fn an_empty_step_is_not_recorded() {
        let mut h = History::default();
        h.push(Step::Cells(Vec::new()));
        h.push(Step::RowsRemoved(Vec::new()));
        h.push(Step::Renamed {
            col: 0,
            before: "A".into(),
            after: "A".into(),
        });
        assert!(h.undo().is_none());
    }

    /// A structural step rides the same stack as a cell edit, and stays replayable in both
    /// directions — the deleted row's cells are on the step, not looked up from the grid.
    #[test]
    fn a_structural_step_undoes_and_redoes() {
        let mut h = History::default();
        let removed = || {
            Step::RowsRemoved(vec![(
                2,
                Row {
                    id: 3,
                    cells: vec!["gone".into()],
                    image: None,
                },
            )])
        };
        h.push(edit(&[(0, 0, "a", "A")]));
        h.push(removed());

        let Some(Step::RowsRemoved(rows)) = h.undo() else {
            panic!("the structural step comes back first");
        };
        assert_eq!(rows[0].0, 2);
        assert_eq!(rows[0].1.cells, vec![SharedString::from("gone")]);

        // ...and the cell edit under it is still there, unshifted.
        assert_eq!(befores(&h.undo().unwrap()), vec![(0, 0, "a".into())]);
        assert!(matches!(h.redo(), Some(Step::Cells(_))));
        assert!(matches!(h.redo(), Some(Step::RowsRemoved(_))));
    }
}
