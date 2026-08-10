//! Undo/redo for grid edits. Deliberately not a command trait: every editing path in the crate
//! funnels through `QrateTableDelegate::apply_edit`, which records the cells it touched here, so a
//! new editing feature gets undo by calling that one function rather than by implementing anything.

use gpui::SharedString;

/// A batch of cell writes as `(row, col, text)` — what `apply_edit` takes and what undo hands back.
pub(crate) type Cells = Vec<(usize, usize, SharedString)>;

/// How many edits to keep. Past this the oldest is dropped — a grid edit holds two strings, so the
/// cap is about bounding a pathological paste loop, not about memory pressure in normal use.
const CAP: usize = 200;

/// One undoable change: every cell it touched as `(row, col, before, after)`. A paste or a bulk
/// fill is a single `Edit`, so it undoes in one step.
pub(crate) struct Edit(pub Vec<(usize, usize, SharedString, SharedString)>);

#[derive(Default)]
pub(crate) struct History {
    done: Vec<Edit>,
    undone: Vec<Edit>,
}

impl History {
    /// Record a change. Drops the redo tail: a new edit forks the timeline.
    pub(crate) fn push(&mut self, edit: Edit) {
        if edit.0.is_empty() {
            return;
        }
        self.undone.clear();
        self.done.push(edit);
        if self.done.len() > CAP {
            self.done.remove(0);
        }
    }

    /// The cells to restore to undo the last edit, as `(row, col, text)`. `None` when there is
    /// nothing left to undo.
    pub(crate) fn undo(&mut self) -> Option<Cells> {
        let edit = self.done.pop()?;
        let cells = edit
            .0
            .iter()
            .map(|(r, c, before, _)| (*r, *c, before.clone()))
            .collect();
        self.undone.push(edit);
        Some(cells)
    }

    /// [`undo`](Self::undo)'s mirror: the cells to restore to redo what was last undone.
    pub(crate) fn redo(&mut self) -> Option<Cells> {
        let edit = self.undone.pop()?;
        let cells = edit
            .0
            .iter()
            .map(|(r, c, _, after)| (*r, *c, after.clone()))
            .collect();
        self.done.push(edit);
        Some(cells)
    }
}

#[cfg(test)]
mod tests {
    use super::{Edit, History};
    use gpui::SharedString;

    fn edit(cells: &[(usize, usize, &str, &str)]) -> Edit {
        Edit(
            cells
                .iter()
                .map(|(r, c, b, a)| (*r, *c, SharedString::from(b.to_string()), (*a).into()))
                .collect(),
        )
    }

    #[test]
    fn edits_undo_in_reverse_order_and_redo_forward() {
        let mut h = History::default();
        h.push(edit(&[(0, 0, "a", "A")]));
        h.push(edit(&[(1, 0, "b", "B")]));
        h.push(edit(&[(2, 0, "c", "C")]));

        assert_eq!(h.undo().unwrap(), vec![(2, 0, "c".into())]);
        assert_eq!(h.undo().unwrap(), vec![(1, 0, "b".into())]);
        assert_eq!(h.redo().unwrap(), vec![(1, 0, "B".into())]);
        assert_eq!(h.redo().unwrap(), vec![(2, 0, "C".into())]);
        assert!(h.redo().is_none());
    }

    #[test]
    fn a_new_edit_after_an_undo_drops_the_redo_tail() {
        let mut h = History::default();
        h.push(edit(&[(0, 0, "a", "A")]));
        h.undo().unwrap();
        h.push(edit(&[(9, 9, "x", "X")]));
        assert!(h.redo().is_none());
        assert_eq!(h.undo().unwrap(), vec![(9, 9, "x".into())]);
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
            h.undo().unwrap(),
            vec![(0, 0, "a".into()), (0, 1, "b".into()), (1, 0, "c".into())]
        );
        assert!(h.undo().is_none());
    }

    #[test]
    fn an_empty_edit_is_not_recorded() {
        let mut h = History::default();
        h.push(Edit(Vec::new()));
        assert!(h.undo().is_none());
    }
}
