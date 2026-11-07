//! The split-pane layout of a tab.
//!
//! A [`Layout`] is an n-ary tree. Leaves are panes. Each split arranges its children along
//! one [`Axis`], and each child gets a `ratio` (a fraction of the split's size).
//!
//! # Invariants
//! The constructors and operations always maintain these:
//! 1. A layout holds at least one pane, and every pane id appears exactly once.
//! 2. Every split has at least two children.
//! 3. Every ratio is finite and positive, and the ratios in a split sum to 1.
//! 4. A split never has a direct child split on the same axis. Such children are
//!    flattened into the parent, so "split right" three times gives one row of four panes.
//!
//! Layouts are plain data. The GUI turns them into rectangles with [`Layout::geometry`],
//! and the daemon applies user actions through the methods in [`ops`].

mod geometry;
mod ops;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use geometry::{Divider, Geometry, PaneRect, Rect};

use crate::PaneId;

/// Smallest ratio a user action may shrink a pane to, within its split.
pub const MIN_RATIO: f64 = 0.05;

/// How far ratios may drift from summing to exactly 1 before a layout is rejected.
const RATIO_TOLERANCE: f64 = 1e-6;

/// The direction in which a split arranges its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Side by side, left to right. Dividers are vertical lines.
    Row,
    /// Stacked, top to bottom. Dividers are horizontal lines.
    Column,
}

/// A direction on screen, used for splitting, resizing and moving focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Towards the left edge.
    Left,
    /// Towards the right edge.
    Right,
    /// Towards the top edge.
    Up,
    /// Towards the bottom edge.
    Down,
}

impl Direction {
    /// The axis this direction moves along.
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Row,
            Self::Up | Self::Down => Axis::Column,
        }
    }

    /// `true` for directions that go towards the end of the axis (right or down).
    pub const fn is_forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }

    /// The forward direction (right or down) along `axis`.
    pub const fn forward(axis: Axis) -> Self {
        match axis {
            Axis::Row => Self::Right,
            Axis::Column => Self::Down,
        }
    }
}

/// One node of the layout tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// A single terminal pane.
    Pane {
        /// The pane shown in this leaf.
        id: PaneId,
    },
    /// Children arranged along `axis`.
    Split {
        /// The direction the children are arranged in.
        axis: Axis,
        /// The children, in order (left to right, or top to bottom).
        children: Vec<Child>,
    },
}

/// A child of a split and its share of the split's size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Child {
    /// Fraction of the parent split's size along its axis, in `(0, 1]`.
    pub ratio: f64,
    /// The child subtree.
    pub node: Node,
}

/// Why a layout operation or construction was rejected.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayoutError {
    /// The referenced pane is not in this layout.
    #[error("pane {0} is not in this layout")]
    PaneNotFound(PaneId),
    /// The pane id is already in this layout.
    #[error("pane {0} already exists in this layout")]
    DuplicatePane(PaneId),
    /// A layout must keep at least one pane. To get rid of the last pane, close the tab.
    #[error("cannot close the last pane of a layout")]
    LastPane,
    /// Splitting would make a pane smaller than [`MIN_RATIO`].
    #[error("pane is too small to split")]
    TooSmall,
    /// There is no divider in that direction to move (the pane touches the layout edge).
    #[error("pane has no adjacent edge in that direction")]
    NoAdjacentEdge,
    /// A split has fewer than two children.
    #[error("split must have at least two children")]
    UnderfilledSplit,
    /// A split directly contains a split on the same axis. Only reported by
    /// [`Layout::check_invariants`]; constructors flatten this automatically.
    #[error("split directly contains a split on the same axis")]
    NestedSameAxis,
    /// A ratio or amount was not finite and positive, or ratios did not sum to 1.
    #[error("invalid ratio: {0}")]
    InvalidRatio(&'static str),
}

/// The split-pane layout of a tab. See the module docs for its invariants.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Node", into = "Node")]
pub struct Layout {
    root: Node,
}

impl Layout {
    /// A layout containing a single pane.
    pub const fn single(pane: PaneId) -> Self {
        Self {
            root: Node::Pane { id: pane },
        }
    }

    /// Builds a layout from a tree (e.g. loaded from storage). The tree is checked
    /// against invariants 1–3, and same-axis nesting is flattened.
    pub fn from_root(root: Node) -> Result<Self, LayoutError> {
        validate(&root, &mut HashSet::new())?;
        Ok(Self {
            root: normalize(root),
        })
    }

    /// The root of the tree, for read-only traversal (e.g. protocol conversion).
    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// All panes, in depth-first order (left-to-right, top-to-bottom reading order).
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        collect_panes(&self.root, &mut out);
        out
    }

    /// Number of panes.
    pub fn len(&self) -> usize {
        count_panes(&self.root)
    }

    /// Always `false`: a layout holds at least one pane. Provided for API completeness.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Whether `pane` is in this layout.
    pub fn contains(&self, pane: PaneId) -> bool {
        path_to(&self.root, pane).is_some()
    }

    /// The first pane in reading order.
    pub fn first_pane(&self) -> PaneId {
        first_leaf(&self.root)
    }

    /// Checks every invariant (including flattening). Operations keep these
    /// automatically, so this is meant for tests and debug assertions.
    pub fn check_invariants(&self) -> Result<(), LayoutError> {
        validate(&self.root, &mut HashSet::new())?;
        if has_same_axis_nesting(&self.root, None) {
            return Err(LayoutError::NestedSameAxis);
        }
        Ok(())
    }
}

impl TryFrom<Node> for Layout {
    type Error = LayoutError;

    fn try_from(root: Node) -> Result<Self, Self::Error> {
        Self::from_root(root)
    }
}

impl From<Layout> for Node {
    fn from(layout: Layout) -> Self {
        layout.root
    }
}

// ---- tree helpers (shared by the submodules) -------------------------------------------

fn validate(node: &Node, seen: &mut HashSet<PaneId>) -> Result<(), LayoutError> {
    match node {
        Node::Pane { id } => {
            if seen.insert(*id) {
                Ok(())
            } else {
                Err(LayoutError::DuplicatePane(*id))
            }
        }
        Node::Split { children, .. } => {
            if children.len() < 2 {
                return Err(LayoutError::UnderfilledSplit);
            }
            let mut sum = 0.0;
            for child in children {
                if !child.ratio.is_finite() || child.ratio <= 0.0 {
                    return Err(LayoutError::InvalidRatio(
                        "ratios must be finite and positive",
                    ));
                }
                sum += child.ratio;
                validate(&child.node, seen)?;
            }
            if (sum - 1.0).abs() > RATIO_TOLERANCE {
                return Err(LayoutError::InvalidRatio("ratios in a split must sum to 1"));
            }
            Ok(())
        }
    }
}

fn has_same_axis_nesting(node: &Node, parent_axis: Option<Axis>) -> bool {
    match node {
        Node::Pane { .. } => false,
        Node::Split { axis, children } => {
            parent_axis == Some(*axis)
                || children
                    .iter()
                    .any(|c| has_same_axis_nesting(&c.node, Some(*axis)))
        }
    }
}

/// Restores invariants 2 and 4: collapses single-child splits and flattens same-axis
/// nesting. Ratios are scaled so each split still sums to 1.
fn normalize(node: Node) -> Node {
    let Node::Split { axis, children } = node else {
        return node;
    };
    let mut out: Vec<Child> = Vec::with_capacity(children.len());
    for Child { ratio, node } in children {
        match normalize(node) {
            Node::Split {
                axis: inner_axis,
                children: inner,
            } if inner_axis == axis => {
                out.extend(inner.into_iter().map(|c| Child {
                    ratio: ratio * c.ratio,
                    node: c.node,
                }));
            }
            node => out.push(Child { ratio, node }),
        }
    }
    if out.len() == 1
        && let Some(only) = out.pop()
    {
        return only.node;
    }
    rescale(&mut out);
    Node::Split {
        axis,
        children: out,
    }
}

/// Scales ratios so they sum to exactly 1. If the ratios are degenerate, they are made equal.
fn rescale(children: &mut [Child]) {
    let sum: f64 = children.iter().map(|c| c.ratio).sum();
    if sum.is_finite() && sum > 0.0 {
        for child in children.iter_mut() {
            child.ratio /= sum;
        }
    } else {
        let equal = 1.0 / count_as_f64(children.len());
        for child in children.iter_mut() {
            child.ratio = equal;
        }
    }
}

/// Converts a child count to `f64`. Child counts are tiny, so this is exact.
#[allow(clippy::cast_precision_loss)]
fn count_as_f64(n: usize) -> f64 {
    n as f64
}

/// Child indices leading from the root to `pane`, or `None` if it is absent.
fn path_to(node: &Node, pane: PaneId) -> Option<Vec<usize>> {
    match node {
        Node::Pane { id } => (*id == pane).then(Vec::new),
        Node::Split { children, .. } => children.iter().enumerate().find_map(|(i, c)| {
            path_to(&c.node, pane).map(|mut rest| {
                rest.insert(0, i);
                rest
            })
        }),
    }
}

fn node_at<'a>(node: &'a Node, path: &[usize]) -> Option<&'a Node> {
    match path.split_first() {
        None => Some(node),
        Some((&i, rest)) => match node {
            Node::Split { children, .. } => node_at(&children.get(i)?.node, rest),
            Node::Pane { .. } => None,
        },
    }
}

fn node_at_mut<'a>(node: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    match path.split_first() {
        None => Some(node),
        Some((&i, rest)) => match node {
            Node::Split { children, .. } => node_at_mut(&mut children.get_mut(i)?.node, rest),
            Node::Pane { .. } => None,
        },
    }
}

fn collect_panes(node: &Node, out: &mut Vec<PaneId>) {
    match node {
        Node::Pane { id } => out.push(*id),
        Node::Split { children, .. } => children.iter().for_each(|c| collect_panes(&c.node, out)),
    }
}

fn count_panes(node: &Node) -> usize {
    match node {
        Node::Pane { .. } => 1,
        Node::Split { children, .. } => children.iter().map(|c| count_panes(&c.node)).sum(),
    }
}

fn first_leaf(node: &Node) -> PaneId {
    let mut node = node;
    loop {
        match node {
            Node::Pane { id } => return *id,
            Node::Split { children, .. } => match children.first() {
                Some(child) => node = &child.node,
                // Unreachable for valid layouts (splits always have ≥ 2 children).
                None => return PaneId::from_u128(0),
            },
        }
    }
}

/// The last leaf in reading order. Within any subtree it touches both the right and the
/// bottom edge, which is why it works as a divider anchor.
fn last_leaf(node: &Node) -> PaneId {
    let mut node = node;
    loop {
        match node {
            Node::Pane { id } => return *id,
            Node::Split { children, .. } => match children.last() {
                Some(child) => node = &child.node,
                None => return PaneId::from_u128(0),
            },
        }
    }
}

#[cfg(test)]
mod tests;
