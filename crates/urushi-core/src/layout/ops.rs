//! Layout operations that change the tree: split, close, resize, move a divider, equalize.
//!
//! Every operation either succeeds and leaves all module invariants true, or fails and
//! leaves the layout unchanged.

use super::{
    Axis, Child, Direction, Layout, LayoutError, MIN_RATIO, Node, first_leaf, last_leaf, node_at,
    node_at_mut, normalize, path_to, rescale,
};
use crate::PaneId;

impl Layout {
    /// Splits `target` and places the new pane `new` on its `side`.
    ///
    /// `Direction::Right` means "split right": `new` appears to the right of `target`.
    /// If `target` already sits in a split on the same axis, `new` becomes a sibling and
    /// takes half of `target`'s space. Otherwise `target` turns into a new two-pane split.
    pub fn split(
        &mut self,
        target: PaneId,
        new: PaneId,
        side: Direction,
    ) -> Result<(), LayoutError> {
        if self.contains(new) {
            return Err(LayoutError::DuplicatePane(new));
        }
        let path = path_to(&self.root, target).ok_or(LayoutError::PaneNotFound(target))?;
        let axis = side.axis();
        let after = side.is_forward();

        let Some((&index, parent_path)) = path.split_last() else {
            // The root itself is the target pane.
            self.root = pair(axis, target, new, after);
            return Ok(());
        };
        let Some(Node::Split {
            axis: parent_axis,
            children,
        }) = node_at_mut(&mut self.root, parent_path)
        else {
            return Err(LayoutError::PaneNotFound(target));
        };
        let Some(slot) = children.get_mut(index) else {
            return Err(LayoutError::PaneNotFound(target));
        };

        if *parent_axis == axis {
            let half = slot.ratio / 2.0;
            if half < MIN_RATIO {
                return Err(LayoutError::TooSmall);
            }
            slot.ratio = half;
            let at = if after { index + 1 } else { index };
            children.insert(
                at,
                Child {
                    ratio: half,
                    node: Node::Pane { id: new },
                },
            );
        } else {
            slot.node = pair(axis, target, new, after);
        }
        Ok(())
    }

    /// Removes `pane`. Its space goes to the adjacent sibling (the previous one, or the
    /// next one if it was first). Returns the pane that should take focus.
    pub fn close(&mut self, pane: PaneId) -> Result<PaneId, LayoutError> {
        let path = path_to(&self.root, pane).ok_or(LayoutError::PaneNotFound(pane))?;
        let Some((&index, parent_path)) = path.split_last() else {
            return Err(LayoutError::LastPane);
        };
        let Some(Node::Split { children, .. }) = node_at_mut(&mut self.root, parent_path) else {
            return Err(LayoutError::PaneNotFound(pane));
        };
        if index >= children.len() {
            return Err(LayoutError::PaneNotFound(pane));
        }

        let removed = children.remove(index);
        let (heir_index, successor) = if index > 0 {
            (
                index - 1,
                children.get(index - 1).map(|c| last_leaf(&c.node)),
            )
        } else {
            (0, children.first().map(|c| first_leaf(&c.node)))
        };
        if let Some(heir) = children.get_mut(heir_index) {
            heir.ratio += removed.ratio;
        }
        rescale(children);

        let root = std::mem::replace(&mut self.root, Node::Pane { id: pane });
        self.root = normalize(root);
        successor.ok_or(LayoutError::PaneNotFound(pane))
    }

    /// Moves `pane`'s edge on `side` outwards by `amount` (a fraction of the enclosing
    /// split's size). A negative `amount` moves it inwards. The result is clamped, so
    /// neither neighbour shrinks below [`MIN_RATIO`].
    pub fn resize(
        &mut self,
        pane: PaneId,
        side: Direction,
        amount: f64,
    ) -> Result<(), LayoutError> {
        if !amount.is_finite() {
            return Err(LayoutError::InvalidRatio("resize amount must be finite"));
        }
        let (split_path, index) = self.edge(pane, side)?;
        let neighbour = if side.is_forward() {
            index + 1
        } else {
            index - 1
        };
        let children = self.children_at_mut(&split_path)?;
        transfer(children, index, neighbour, amount)
    }

    /// Places the divider after `anchor` (on `axis`) at `position`. `position` is a fraction
    /// of the enclosing split's size, measured from its start. Built for mouse dragging:
    /// pass the [`super::Divider`]'s `anchor` and `axis` from [`Layout::geometry`].
    pub fn move_divider(
        &mut self,
        anchor: PaneId,
        axis: Axis,
        position: f64,
    ) -> Result<(), LayoutError> {
        if !position.is_finite() {
            return Err(LayoutError::InvalidRatio("divider position must be finite"));
        }
        let (split_path, index) = self.edge(anchor, Direction::forward(axis))?;
        let children = self.children_at_mut(&split_path)?;
        let start: f64 = children.iter().take(index).map(|c| c.ratio).sum();
        let current = start + children.get(index).map_or(0.0, |c| c.ratio);
        transfer(children, index, index + 1, position - current)
    }

    /// Gives every child of every split an equal share.
    pub fn equalize(&mut self) {
        equalize_node(&mut self.root);
    }

    /// Finds the split and child index where the branch containing `pane` has a
    /// neighbour towards `side`. Searches from the innermost enclosing split outwards.
    fn edge(&self, pane: PaneId, side: Direction) -> Result<(Vec<usize>, usize), LayoutError> {
        let path = path_to(&self.root, pane).ok_or(LayoutError::PaneNotFound(pane))?;
        for depth in (0..path.len()).rev() {
            let ancestor_path = &path[..depth];
            let index = path[depth];
            if let Some(Node::Split { axis, children }) = node_at(&self.root, ancestor_path) {
                let has_neighbour = if side.is_forward() {
                    index + 1 < children.len()
                } else {
                    index > 0
                };
                if *axis == side.axis() && has_neighbour {
                    return Ok((ancestor_path.to_vec(), index));
                }
            }
        }
        Err(LayoutError::NoAdjacentEdge)
    }

    fn children_at_mut(&mut self, path: &[usize]) -> Result<&mut Vec<Child>, LayoutError> {
        match node_at_mut(&mut self.root, path) {
            Some(Node::Split { children, .. }) => Ok(children),
            _ => Err(LayoutError::NoAdjacentEdge),
        }
    }
}

/// A two-pane split of `existing` and `new`, half each, with `new` after `existing` if `after`.
fn pair(axis: Axis, existing: PaneId, new: PaneId, after: bool) -> Node {
    let (first, second) = if after {
        (existing, new)
    } else {
        (new, existing)
    };
    Node::Split {
        axis,
        children: vec![
            Child {
                ratio: 0.5,
                node: Node::Pane { id: first },
            },
            Child {
                ratio: 0.5,
                node: Node::Pane { id: second },
            },
        ],
    }
}

/// Moves `amount` of ratio from `shrink` to `grow` (negative amounts go the other way).
/// Neither child goes below `MIN_RATIO`, or below its current size if it is already
/// smaller than that.
fn transfer(
    children: &mut [Child],
    grow: usize,
    shrink: usize,
    amount: f64,
) -> Result<(), LayoutError> {
    let (Some(g), Some(s)) = (
        children.get(grow).map(|c| c.ratio),
        children.get(shrink).map(|c| c.ratio),
    ) else {
        return Err(LayoutError::NoAdjacentEdge);
    };
    let total = g + s;
    let lower = MIN_RATIO.min(g);
    let upper = total - MIN_RATIO.min(s);
    // Not `clamp`: when both sides are already tiny, rounding can put `upper` a hair
    // below `lower`, and `f64::clamp` panics on that. Found by the property tests.
    let new_grow = (g + amount).min(upper).max(lower);
    if let Some(c) = children.get_mut(grow) {
        c.ratio = new_grow;
    }
    if let Some(c) = children.get_mut(shrink) {
        c.ratio = total - new_grow;
    }
    rescale(children);
    Ok(())
}

fn equalize_node(node: &mut Node) {
    if let Node::Split { children, .. } = node {
        let share = 1.0 / super::count_as_f64(children.len());
        for child in children.iter_mut() {
            child.ratio = share;
            equalize_node(&mut child.node);
        }
    }
}
