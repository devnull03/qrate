use std::collections::{HashMap, HashSet};

use settings::project::{RowId, RowStructure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Before(RowId),
    After(RowId),
    ChildOf(RowId),
    Root,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeleteMode {
    Subtree,
    PromoteChildren,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    UnknownRow(RowId),
    CannotMoveIntoDescendant,
    NoPreviousSibling,
    AlreadyRoot,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hierarchy {
    rows: Vec<RowStructure>,
    expanded: HashSet<RowId>,
}

impl Hierarchy {
    pub(crate) fn from_rows(
        row_ids: &[RowId],
        stored: &[RowStructure],
        default_level: &str,
    ) -> Self {
        let stored: HashMap<_, _> = stored.iter().map(|row| (row.row_id, row)).collect();
        let known: HashSet<_> = row_ids.iter().copied().collect();
        let mut next_root = stored
            .values()
            .filter(|row| row.parent_id.is_none())
            .map(|row| row.sibling_order + 1)
            .max()
            .unwrap_or_default();
        let rows = row_ids
            .iter()
            .map(|row_id| match stored.get(row_id) {
                Some(row) => {
                    let mut row = (*row).clone();
                    if row.parent_id.is_some_and(|parent| !known.contains(&parent)) {
                        row.parent_id = None;
                    }
                    row
                }
                None => {
                    let row = RowStructure {
                        row_id: *row_id,
                        parent_id: None,
                        level_key: default_level.into(),
                        sibling_order: next_root,
                        source_path: None,
                        source_kind: None,
                    };
                    next_root += 1;
                    row
                }
            })
            .collect::<Vec<_>>();
        let mut hierarchy = Self {
            rows,
            expanded: HashSet::new(),
        };
        hierarchy.normalize();
        hierarchy
    }

    pub(crate) fn rows(&self) -> &[RowStructure] {
        &self.rows
    }

    pub(crate) fn reconcile(&mut self, row_ids: &[RowId], default_level: &str) {
        let expanded = std::mem::take(&mut self.expanded);
        let mut next = Self::from_rows(row_ids, &self.rows, default_level);
        next.expanded = expanded
            .into_iter()
            .filter(|row_id| row_ids.contains(row_id))
            .collect();
        *self = next;
    }

    pub(crate) fn replace_rows(&mut self, row_ids: &[RowId], rows: &[RowStructure], default: &str) {
        let expanded = std::mem::take(&mut self.expanded);
        let mut next = Self::from_rows(row_ids, rows, default);
        next.expanded = expanded;
        *self = next;
    }

    pub(crate) fn subtree(&self, row_id: RowId) -> Result<Vec<RowId>, Error> {
        self.row(row_id)?;
        let mut rows = vec![row_id];
        rows.extend(self.descendants(row_id));
        Ok(rows)
    }

    pub(crate) fn set_expanded(&mut self, row_id: RowId, expanded: bool) {
        if expanded {
            self.expanded.insert(row_id);
        } else {
            self.expanded.remove(&row_id);
        }
    }

    pub(crate) fn is_expanded(&self, row_id: RowId) -> bool {
        self.expanded.contains(&row_id)
    }

    pub(crate) fn has_children(&self, row_id: RowId) -> bool {
        self.rows.iter().any(|row| row.parent_id == Some(row_id))
    }

    pub(crate) fn expand_all(&mut self) {
        self.expanded = self
            .rows
            .iter()
            .filter(|row| {
                self.rows
                    .iter()
                    .any(|child| child.parent_id == Some(row.row_id))
            })
            .map(|row| row.row_id)
            .collect();
    }

    pub(crate) fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    pub(crate) fn projection(&self, matching: Option<&HashSet<RowId>>) -> Vec<(RowId, usize)> {
        let included = matching.map(|matching| {
            let by_id: HashMap<_, _> = self.rows.iter().map(|row| (row.row_id, row)).collect();
            let mut included = matching.clone();
            for row_id in matching {
                let mut parent = by_id.get(row_id).and_then(|row| row.parent_id);
                while let Some(parent_id) = parent {
                    if !included.insert(parent_id) {
                        break;
                    }
                    parent = by_id.get(&parent_id).and_then(|row| row.parent_id);
                }
            }
            included
        });
        let mut output = Vec::new();
        self.append_children(None, 0, included.as_ref(), &mut output);
        output
    }

    fn append_children(
        &self,
        parent: Option<RowId>,
        depth: usize,
        included: Option<&HashSet<RowId>>,
        output: &mut Vec<(RowId, usize)>,
    ) {
        let mut children: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.parent_id == parent)
            .collect();
        children.sort_by_key(|row| (row.sibling_order, row.row_id));
        for child in children {
            if included.is_none_or(|included| included.contains(&child.row_id)) {
                output.push((child.row_id, depth));
            }
            let reveal_match = included.is_some_and(|included| {
                self.descendants(child.row_id)
                    .iter()
                    .any(|row| included.contains(row))
            });
            if self.expanded.contains(&child.row_id) || reveal_match {
                self.append_children(Some(child.row_id), depth + 1, included, output);
            }
        }
    }

    pub(crate) fn move_row(&mut self, row_id: RowId, placement: Placement) -> Result<(), Error> {
        self.row(row_id)?;
        let (parent, at) = match placement {
            Placement::Root => (None, self.children(None).len()),
            Placement::ChildOf(target) => {
                self.ensure_target(row_id, target)?;
                (Some(target), self.children(Some(target)).len())
            }
            Placement::Before(target) | Placement::After(target) => {
                self.ensure_target(row_id, target)?;
                let target_row = self.row(target)?;
                let siblings = self.children(target_row.parent_id);
                let target_at = siblings.iter().position(|id| *id == target).unwrap_or(0);
                (
                    target_row.parent_id,
                    target_at + usize::from(matches!(placement, Placement::After(_))),
                )
            }
        };
        let row = self.row_mut(row_id)?;
        row.parent_id = parent;
        row.sibling_order = at as i64;
        self.normalize_with_priority(row_id, at);
        Ok(())
    }

    pub(crate) fn indent(&mut self, row_id: RowId) -> Result<(), Error> {
        let row = self.row(row_id)?;
        let siblings = self.children(row.parent_id);
        let at = siblings.iter().position(|id| *id == row_id).unwrap_or(0);
        let previous = at
            .checked_sub(1)
            .and_then(|at| siblings.get(at))
            .copied()
            .ok_or(Error::NoPreviousSibling)?;
        self.move_row(row_id, Placement::ChildOf(previous))
    }

    pub(crate) fn outdent(&mut self, row_id: RowId) -> Result<(), Error> {
        let parent = self.row(row_id)?.parent_id.ok_or(Error::AlreadyRoot)?;
        self.move_row(row_id, Placement::After(parent))
    }

    pub(crate) fn delete(&mut self, row_id: RowId, mode: DeleteMode) -> Result<Vec<RowId>, Error> {
        let row = self.row(row_id)?.clone();
        let removed = match mode {
            DeleteMode::Subtree => {
                let mut removed = self.descendants(row_id);
                removed.push(row_id);
                let removed_set: HashSet<_> = removed.iter().copied().collect();
                self.rows.retain(|row| !removed_set.contains(&row.row_id));
                removed
            }
            DeleteMode::PromoteChildren => {
                for child in self
                    .rows
                    .iter_mut()
                    .filter(|child| child.parent_id == Some(row_id))
                {
                    child.parent_id = row.parent_id;
                }
                self.rows.retain(|row| row.row_id != row_id);
                vec![row_id]
            }
        };
        for removed in &removed {
            self.expanded.remove(removed);
        }
        self.normalize();
        Ok(removed)
    }

    fn row(&self, row_id: RowId) -> Result<&RowStructure, Error> {
        self.rows
            .iter()
            .find(|row| row.row_id == row_id)
            .ok_or(Error::UnknownRow(row_id))
    }

    fn row_mut(&mut self, row_id: RowId) -> Result<&mut RowStructure, Error> {
        self.rows
            .iter_mut()
            .find(|row| row.row_id == row_id)
            .ok_or(Error::UnknownRow(row_id))
    }

    fn children(&self, parent: Option<RowId>) -> Vec<RowId> {
        let mut children: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.parent_id == parent)
            .map(|row| (row.sibling_order, row.row_id))
            .collect();
        children.sort_unstable();
        children.into_iter().map(|(_, row_id)| row_id).collect()
    }

    fn descendants(&self, row_id: RowId) -> Vec<RowId> {
        let mut descendants = Vec::new();
        let mut pending = self.children(Some(row_id));
        while let Some(child) = pending.pop() {
            pending.extend(self.children(Some(child)));
            descendants.push(child);
        }
        descendants
    }

    fn ensure_target(&self, row_id: RowId, target: RowId) -> Result<(), Error> {
        self.row(target)?;
        if row_id == target || self.descendants(row_id).contains(&target) {
            return Err(Error::CannotMoveIntoDescendant);
        }
        Ok(())
    }

    fn normalize_with_priority(&mut self, row_id: RowId, at: usize) {
        let parent = self.row(row_id).ok().and_then(|row| row.parent_id);
        let mut siblings = self.children(parent);
        siblings.retain(|id| *id != row_id);
        siblings.insert(at.min(siblings.len()), row_id);
        for (order, sibling) in siblings.into_iter().enumerate() {
            if let Ok(row) = self.row_mut(sibling) {
                row.sibling_order = order as i64;
            }
        }
        self.normalize();
    }

    fn normalize(&mut self) {
        let parents: HashSet<_> = self.rows.iter().map(|row| row.parent_id).collect();
        for parent in parents {
            let children = self.children(parent);
            for (order, child) in children.into_iter().enumerate() {
                if let Ok(row) = self.row_mut(child) {
                    row.sibling_order = order as i64;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hierarchy() -> Hierarchy {
        let rows = [
            (1, None, 0),
            (2, Some(1), 0),
            (3, Some(1), 1),
            (4, Some(3), 0),
            (5, None, 1),
        ]
        .map(|(row_id, parent_id, sibling_order)| RowStructure {
            row_id,
            parent_id,
            level_key: "item".into(),
            sibling_order,
            source_path: None,
            source_kind: None,
        });
        Hierarchy::from_rows(&[1, 2, 3, 4, 5], &rows, "item")
    }

    #[test]
    fn collapse_and_expand_project_the_tree() {
        let mut hierarchy = hierarchy();
        assert_eq!(hierarchy.projection(None), [(1, 0), (5, 0)]);
        hierarchy.set_expanded(1, true);
        assert_eq!(hierarchy.projection(None), [(1, 0), (2, 1), (3, 1), (5, 0)]);
        hierarchy.expand_all();
        assert_eq!(
            hierarchy.projection(None),
            [(1, 0), (2, 1), (3, 1), (4, 2), (5, 0)]
        );
        hierarchy.collapse_all();
        assert_eq!(hierarchy.projection(None), [(1, 0), (5, 0)]);
    }

    #[test]
    fn a_filter_reveals_matching_descendants_and_their_ancestors() {
        let hierarchy = hierarchy();
        assert_eq!(
            hierarchy.projection(Some(&HashSet::from([4]))),
            [(1, 0), (3, 1), (4, 2)]
        );
    }

    #[test]
    fn moving_a_subtree_changes_only_its_parent_and_order() {
        let mut hierarchy = hierarchy();
        hierarchy.move_row(3, Placement::Root).unwrap();
        hierarchy.move_row(3, Placement::Before(5)).unwrap();
        hierarchy.expand_all();
        assert_eq!(
            hierarchy.projection(None),
            [(1, 0), (2, 1), (3, 0), (4, 1), (5, 0)]
        );
        assert_eq!(hierarchy.row(4).unwrap().parent_id, Some(3));
        assert_eq!(hierarchy.rows().len(), 5);
    }

    #[test]
    fn moving_a_parent_into_its_descendant_is_rejected_atomically() {
        let mut hierarchy = hierarchy();
        let before = hierarchy.clone();
        assert_eq!(
            hierarchy.move_row(1, Placement::ChildOf(4)),
            Err(Error::CannotMoveIntoDescendant)
        );
        assert_eq!(hierarchy, before);
    }

    #[test]
    fn indent_and_outdent_use_adjacent_hierarchy_positions() {
        let mut hierarchy = hierarchy();
        hierarchy.indent(3).unwrap();
        assert_eq!(hierarchy.row(3).unwrap().parent_id, Some(2));
        hierarchy.outdent(3).unwrap();
        assert_eq!(hierarchy.row(3).unwrap().parent_id, Some(1));
    }

    #[test]
    fn delete_supports_subtree_and_child_promotion() {
        let mut promoted = hierarchy();
        assert_eq!(
            promoted.delete(3, DeleteMode::PromoteChildren).unwrap(),
            [3]
        );
        assert_eq!(promoted.row(4).unwrap().parent_id, Some(1));

        let mut removed = hierarchy();
        let removed_ids: HashSet<_> = removed
            .delete(3, DeleteMode::Subtree)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(removed_ids, HashSet::from([3, 4]));
        assert!(matches!(removed.row(4), Err(Error::UnknownRow(4))));
    }
}
