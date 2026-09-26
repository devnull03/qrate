use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use settings::project::{RowId, RowStructure, SourceKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Before(RowId),
    After(RowId),
    ChildOf(RowId),
    Root,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    UnknownRow(RowId),
    CannotMoveIntoDescendant,
    NoPreviousSibling,
    AlreadyRoot,
}

/// One row's place in the tree. Its sibling order is not stored: it is the row's position in its
/// parent's `children` list, so moving a row never renumbers its siblings.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub parent: Option<RowId>,
    level_key: String,
    source_path: Option<String>,
    source_kind: Option<SourceKind>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Hierarchy {
    /// Every row, in source order — the order [`rows`](Self::rows) reports them in.
    order: Vec<RowId>,
    nodes: HashMap<RowId, Node>,
    /// Each parent's children in sibling order; `None` holds the roots.
    children: HashMap<Option<RowId>, Vec<RowId>>,
    expanded: HashSet<RowId>,
    /// [`rows`](Self::rows), built on first ask after a structural change and shared by every undo
    /// step that snapshots the same state.
    materialized: OnceCell<Arc<[RowStructure]>>,
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
        let mut placed: HashMap<Option<RowId>, Vec<(i64, RowId)>> = HashMap::new();
        let mut nodes = HashMap::with_capacity(row_ids.len());
        for &row_id in row_ids {
            let (node, order) = match stored.get(&row_id) {
                Some(row) => (
                    Node {
                        parent: row.parent_id.filter(|parent| known.contains(parent)),
                        level_key: row.level_key.clone(),
                        source_path: row.source_path.clone(),
                        source_kind: row.source_kind,
                    },
                    row.sibling_order,
                ),
                None => {
                    next_root += 1;
                    (
                        Node {
                            parent: None,
                            level_key: default_level.into(),
                            source_path: None,
                            source_kind: None,
                        },
                        next_root - 1,
                    )
                }
            };
            placed.entry(node.parent).or_default().push((order, row_id));
            nodes.insert(row_id, node);
        }
        let children = placed
            .into_iter()
            .map(|(parent, mut kids)| {
                kids.sort_unstable();
                (parent, kids.into_iter().map(|(_, id)| id).collect())
            })
            .collect();
        Self {
            order: row_ids.to_vec(),
            nodes,
            children,
            ..Self::default()
        }
    }

    /// Every row's structure in source order, with `sibling_order` read off its position.
    pub(crate) fn rows(&self) -> Arc<[RowStructure]> {
        self.materialized
            .get_or_init(|| {
                let position: HashMap<RowId, i64> = self
                    .children
                    .values()
                    .flat_map(|kids| kids.iter().copied().zip(0..))
                    .collect();
                self.order
                    .iter()
                    .filter_map(|row_id| {
                        let node = self.nodes.get(row_id)?;
                        Some(RowStructure {
                            row_id: *row_id,
                            parent_id: node.parent,
                            level_key: node.level_key.clone(),
                            sibling_order: position.get(row_id).copied().unwrap_or_default(),
                            source_path: node.source_path.clone(),
                            source_kind: node.source_kind,
                        })
                    })
                    .collect()
            })
            .clone()
    }

    fn changed(&mut self) {
        self.materialized = OnceCell::new();
    }

    /// Follow the grid's rows: rows it no longer has leave their children in their place, rows it
    /// gained join the end of the roots.
    pub(crate) fn reconcile(&mut self, row_ids: &[RowId], default_level: &str) {
        let known: HashSet<RowId> = row_ids.iter().copied().collect();
        let gone: Vec<RowId> = self
            .nodes
            .keys()
            .filter(|row_id| !known.contains(row_id))
            .copied()
            .collect();
        self.delete(&gone);
        for &row_id in row_ids {
            if self.nodes.contains_key(&row_id) {
                continue;
            }
            self.nodes.insert(
                row_id,
                Node {
                    parent: None,
                    level_key: default_level.into(),
                    source_path: None,
                    source_kind: None,
                },
            );
            self.children.entry(None).or_default().push(row_id);
        }
        self.order = row_ids.to_vec();
        self.expanded.retain(|row_id| known.contains(row_id));
        self.changed();
    }

    pub(crate) fn replace_rows(&mut self, row_ids: &[RowId], rows: &[RowStructure], default: &str) {
        let expanded = std::mem::take(&mut self.expanded);
        *self = Self::from_rows(row_ids, rows, default);
        self.expanded = expanded;
    }

    pub(crate) fn subtree(&self, row_id: RowId) -> Result<Vec<RowId>, Error> {
        self.node(row_id)?;
        let mut rows = vec![row_id];
        let mut pending = self.children(Some(row_id)).to_vec();
        while let Some(child) = pending.pop() {
            pending.extend_from_slice(self.children(Some(child)));
            rows.push(child);
        }
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

    pub(crate) fn expanded_rows(&self) -> Vec<RowId> {
        let mut rows: Vec<_> = self.expanded.iter().copied().collect();
        rows.sort_unstable();
        rows
    }

    pub(crate) fn restore_expanded(&mut self, row_ids: &[RowId]) {
        self.expanded = row_ids
            .iter()
            .copied()
            .filter(|row_id| self.has_children(*row_id))
            .collect();
    }

    pub(crate) fn child_count(&self, row_id: RowId) -> usize {
        self.children(Some(row_id)).len()
    }

    pub(crate) fn has_children(&self, row_id: RowId) -> bool {
        self.child_count(row_id) > 0
    }

    pub(crate) fn set_level(&mut self, row_id: RowId, level_key: &str) -> Result<(), Error> {
        self.nodes
            .get_mut(&row_id)
            .ok_or(Error::UnknownRow(row_id))?
            .level_key = level_key.into();
        self.changed();
        Ok(())
    }

    /// Point a row at the file it now links to.
    pub(crate) fn set_source_file(&mut self, row_id: RowId, source_path: String) {
        if let Some(node) = self.nodes.get_mut(&row_id) {
            node.source_path = Some(source_path);
            node.source_kind = Some(SourceKind::File);
            self.changed();
        }
    }

    pub(crate) fn level(&self, row_id: RowId) -> Option<&str> {
        self.nodes.get(&row_id).map(|node| node.level_key.as_str())
    }

    pub(crate) fn expand_all(&mut self) {
        self.expanded = self
            .children
            .iter()
            .filter(|(_, kids)| !kids.is_empty())
            .filter_map(|(parent, _)| *parent)
            .collect();
    }

    pub(crate) fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    pub(crate) fn projection(&self, matching: Option<&HashSet<RowId>>) -> Vec<(RowId, usize)> {
        let included = matching.map(|matching| {
            let mut included = matching.clone();
            for row_id in matching {
                let mut parent = self.nodes.get(row_id).and_then(|node| node.parent);
                while let Some(parent_id) = parent {
                    if !included.insert(parent_id) {
                        break;
                    }
                    parent = self.nodes.get(&parent_id).and_then(|node| node.parent);
                }
            }
            included
        });
        let mut output = Vec::with_capacity(self.order.len());
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
        for &child in self.children(parent) {
            if included.is_none_or(|included| included.contains(&child)) {
                output.push((child, depth));
            }
            // `included` holds every ancestor of a match, so a deeper match shows up as an
            // included direct child.
            let reveal_match = included.is_some_and(|included| {
                self.children(Some(child))
                    .iter()
                    .any(|row| included.contains(row))
            });
            if self.expanded.contains(&child) || reveal_match {
                self.append_children(Some(child), depth + 1, included, output);
            }
        }
    }

    pub(crate) fn move_row(&mut self, row_id: RowId, placement: Placement) -> Result<(), Error> {
        self.node(row_id)?;
        let (parent, at) = match placement {
            Placement::Root => (None, self.children(None).len()),
            Placement::ChildOf(target) => {
                self.ensure_target(row_id, target)?;
                (Some(target), self.children(Some(target)).len())
            }
            Placement::Before(target) | Placement::After(target) => {
                self.ensure_target(row_id, target)?;
                let parent = self.node(target)?.parent;
                let target_at = self
                    .children(parent)
                    .iter()
                    .filter(|id| **id != row_id)
                    .position(|id| *id == target)
                    .unwrap_or(0);
                (
                    parent,
                    target_at + usize::from(matches!(placement, Placement::After(_))),
                )
            }
        };
        let old_parent = self.node(row_id)?.parent;
        if let Some(siblings) = self.children.get_mut(&old_parent) {
            siblings.retain(|id| *id != row_id);
        }
        let siblings = self.children.entry(parent).or_default();
        siblings.insert(at.min(siblings.len()), row_id);
        if let Some(node) = self.nodes.get_mut(&row_id) {
            node.parent = parent;
        }
        self.changed();
        Ok(())
    }

    pub(crate) fn indent(&mut self, row_id: RowId) -> Result<(), Error> {
        let siblings = self.children(self.node(row_id)?.parent);
        let at = siblings.iter().position(|id| *id == row_id).unwrap_or(0);
        let previous = at
            .checked_sub(1)
            .and_then(|at| siblings.get(at))
            .copied()
            .ok_or(Error::NoPreviousSibling)?;
        self.move_row(row_id, Placement::ChildOf(previous))
    }

    pub(crate) fn outdent(&mut self, row_id: RowId) -> Result<(), Error> {
        let parent = self.node(row_id)?.parent.ok_or(Error::AlreadyRoot)?;
        self.move_row(row_id, Placement::After(parent))
    }

    /// Removes rows, leaving each one's children where it stood — under the nearest ancestor that
    /// stays. Deleting a whole subtree is the caller passing every row in
    /// [`subtree`](Self::subtree). Unknown rows are ignored.
    pub(crate) fn delete(&mut self, row_ids: &[RowId]) {
        let doomed: HashSet<RowId> = row_ids
            .iter()
            .copied()
            .filter(|row_id| self.nodes.contains_key(row_id))
            .collect();
        if doomed.is_empty() {
            return;
        }
        let mut heirs: HashSet<Option<RowId>> = HashSet::new();
        for row_id in &doomed {
            heirs.insert(self.surviving_parent(*row_id, &doomed));
        }
        for parent in heirs {
            let before = self.children.remove(&parent).unwrap_or_default();
            let mut after = Vec::with_capacity(before.len());
            promote(&mut self.children, &before, &doomed, &mut after);
            for row_id in &after {
                if let Some(node) = self.nodes.get_mut(row_id) {
                    node.parent = parent;
                }
            }
            if !after.is_empty() {
                self.children.insert(parent, after);
            }
        }
        for row_id in &doomed {
            self.nodes.remove(row_id);
            self.children.remove(&Some(*row_id));
            self.expanded.remove(row_id);
        }
        self.order.retain(|row_id| !doomed.contains(row_id));
        self.changed();
    }

    /// The nearest ancestor of `row_id` that is not being deleted, `None` for the roots.
    fn surviving_parent(&self, row_id: RowId, doomed: &HashSet<RowId>) -> Option<RowId> {
        let mut parent = self.nodes.get(&row_id).and_then(|node| node.parent);
        for _ in 0..self.nodes.len() {
            match parent {
                Some(id) if doomed.contains(&id) => {
                    parent = self.nodes.get(&id).and_then(|node| node.parent)
                }
                _ => break,
            }
        }
        parent
    }

    pub(crate) fn node(&self, row_id: RowId) -> Result<&Node, Error> {
        self.nodes.get(&row_id).ok_or(Error::UnknownRow(row_id))
    }

    pub(crate) fn children(&self, parent: Option<RowId>) -> &[RowId] {
        self.children
            .get(&parent)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Walks up from `target`: a row's depth is small, while its subtree can be the whole table.
    fn ensure_target(&self, row_id: RowId, target: RowId) -> Result<(), Error> {
        self.node(target)?;
        let mut ancestor = Some(target);
        for _ in 0..=self.nodes.len() {
            match ancestor {
                Some(id) if id == row_id => return Err(Error::CannotMoveIntoDescendant),
                Some(id) => ancestor = self.nodes.get(&id).and_then(|node| node.parent),
                None => break,
            }
        }
        Ok(())
    }
}

/// `kids` with every doomed row replaced by its own children, recursively, in place.
fn promote(
    children: &mut HashMap<Option<RowId>, Vec<RowId>>,
    kids: &[RowId],
    doomed: &HashSet<RowId>,
    into: &mut Vec<RowId>,
) {
    for &row_id in kids {
        if doomed.contains(&row_id) {
            let grandchildren = children.remove(&Some(row_id)).unwrap_or_default();
            promote(children, &grandchildren, doomed, into);
        } else {
            into.push(row_id);
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
    fn restored_expansion_ignores_stale_and_leaf_rows() {
        let mut hierarchy = hierarchy();
        hierarchy.restore_expanded(&[1, 2, 3, 99]);
        assert_eq!(hierarchy.expanded_rows(), [1, 3]);
        assert_eq!(
            hierarchy.projection(None),
            [(1, 0), (2, 1), (3, 1), (4, 2), (5, 0)]
        );
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
        assert_eq!(hierarchy.node(4).unwrap().parent, Some(3));
        assert_eq!(hierarchy.rows().len(), 5);
    }

    #[test]
    fn moving_a_row_down_lands_beside_its_target() {
        for placement in [Placement::After(5), Placement::Before(3)] {
            let mut hierarchy = hierarchy();
            hierarchy.move_row(3, Placement::Root).unwrap();
            hierarchy.move_row(1, placement).unwrap();
            assert_eq!(hierarchy.children(None), [5, 1, 3]);
        }
    }

    #[test]
    fn moving_a_parent_into_its_descendant_is_rejected_atomically() {
        let mut hierarchy = hierarchy();
        let before = hierarchy.rows();
        assert_eq!(
            hierarchy.move_row(1, Placement::ChildOf(4)),
            Err(Error::CannotMoveIntoDescendant)
        );
        assert_eq!(hierarchy.rows(), before);
    }

    #[test]
    fn indent_and_outdent_use_adjacent_hierarchy_positions() {
        let mut hierarchy = hierarchy();
        hierarchy.indent(3).unwrap();
        assert_eq!(hierarchy.node(3).unwrap().parent, Some(2));
        hierarchy.outdent(3).unwrap();
        assert_eq!(hierarchy.node(3).unwrap().parent, Some(1));
    }

    #[test]
    fn delete_promotes_children_into_their_parent_s_place() {
        let mut promoted = hierarchy();
        promoted.delete(&[3]);
        assert_eq!(promoted.node(4).unwrap().parent, Some(1));
        // 4 takes 3's place after 2 rather than joining the end of the list.
        assert_eq!(promoted.children(Some(1)), [2, 4]);
    }

    /// Deleting a parent together with its child in one pass promotes the grandchild past both.
    #[test]
    fn deleting_a_chain_promotes_to_the_nearest_survivor() {
        let mut hierarchy = hierarchy();
        hierarchy.delete(&[1, 3]);
        assert_eq!(hierarchy.children(None), [2, 4, 5]);
        assert_eq!(hierarchy.node(4).unwrap().parent, None);
        assert_eq!(
            hierarchy
                .rows()
                .iter()
                .map(|row| (row.row_id, row.sibling_order))
                .collect::<Vec<_>>(),
            [(2, 0), (4, 1), (5, 2)]
        );
    }

    /// The sizes the positional layout exists for: a flat table of ten thousand rows, edited at
    /// both ends and in bulk, still reports every row once, in order, with contiguous sibling order.
    #[test]
    fn a_ten_thousand_row_flat_table_inserts_and_deletes_correctly() {
        const ROWS: RowId = 10_000;
        let ids: Vec<RowId> = (1..=ROWS).collect();
        let mut hierarchy = Hierarchy::from_rows(&ids, &[], "item");
        assert_eq!(hierarchy.children(None), ids.as_slice());

        let odd: Vec<RowId> = ids.iter().copied().filter(|id| id % 2 == 1).collect();
        hierarchy.delete(&odd);
        let even: Vec<RowId> = ids.iter().copied().filter(|id| id % 2 == 0).collect();
        assert_eq!(hierarchy.children(None), even.as_slice());

        let mut grown = even.clone();
        grown.push(ROWS + 1);
        hierarchy.reconcile(&grown, "item");
        hierarchy.move_row(ROWS + 1, Placement::Before(2)).unwrap();
        hierarchy.move_row(4, Placement::ChildOf(2)).unwrap();

        let rows = hierarchy.rows();
        assert_eq!(rows.len(), grown.len());
        assert_eq!(
            rows.iter().map(|row| row.row_id).collect::<Vec<_>>(),
            grown,
            "rows stay in source order"
        );
        let roots = hierarchy.children(None);
        assert_eq!(&roots[..3], [ROWS + 1, 2, 6]);
        assert_eq!(roots.len(), grown.len() - 1);
        assert_eq!(hierarchy.children(Some(2)), [4]);
        let mut orders: Vec<i64> = rows
            .iter()
            .filter(|row| row.parent_id.is_none())
            .map(|row| row.sibling_order)
            .collect();
        orders.sort_unstable();
        assert_eq!(orders, (0..roots.len() as i64).collect::<Vec<_>>());
        assert_eq!(
            hierarchy.projection(None).len(),
            roots.len(),
            "the collapsed child stays hidden"
        );
    }
}
