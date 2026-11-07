//! Turning a layout into rectangles, and moving focus between panes by direction.
//!
//! Geometry is computed in whatever units the caller passes in: pixels in the GUI, or
//! the unit square for direction-based navigation. Divider *thickness* is up to the
//! renderer. Here, dividers are zero-width lines between neighbouring children.

use super::{Axis, Direction, Layout, Node, last_leaf};
use crate::PaneId;

/// Tolerance for comparing edges that should touch.
const EDGE_EPSILON: f64 = 1e-9;

/// An axis-aligned rectangle. `x` grows to the right and `y` grows downwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width (≥ 0).
    pub width: f64,
    /// Height (≥ 0).
    pub height: f64,
}

impl Rect {
    /// The unit square `(0, 0, 1, 1)`.
    pub const UNIT: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    /// Right edge.
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn span(&self, axis: Axis) -> (f64, f64) {
        match axis {
            Axis::Row => (self.x, self.width),
            Axis::Column => (self.y, self.height),
        }
    }

    fn with_span(&self, axis: Axis, start: f64, length: f64) -> Self {
        match axis {
            Axis::Row => Self {
                x: start,
                width: length,
                ..*self
            },
            Axis::Column => Self {
                y: start,
                height: length,
                ..*self
            },
        }
    }
}

/// Where one pane is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneRect {
    /// The pane.
    pub pane: PaneId,
    /// Its rectangle.
    pub rect: Rect,
}

/// A draggable boundary between two neighbouring children of a split.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divider {
    /// A pane just before the divider. Pass it to [`Layout::move_divider`].
    pub anchor: PaneId,
    /// The axis of the split this divider belongs to.
    pub axis: Axis,
    /// The whole split's rectangle. Used to turn a cursor position into a fraction.
    pub split: Rect,
    /// The divider's position as a fraction of `split` along `axis`.
    pub position: f64,
}

/// Every pane rectangle and divider in a layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    /// Pane rectangles, in reading order.
    pub panes: Vec<PaneRect>,
    /// Dividers, outer splits first.
    pub dividers: Vec<Divider>,
}

impl Layout {
    /// Lays the tree out inside `bounds`.
    pub fn geometry(&self, bounds: Rect) -> Geometry {
        let mut out = Geometry::default();
        layout_node(&self.root, bounds, &mut out);
        out
    }

    /// The pane that focus should move to when going from `pane` towards `side`, or
    /// `None` at the layout's edge.
    ///
    /// The choice is spatial, not by tree position. Among panes whose edge touches
    /// `pane`'s `side` edge, it picks the one that overlaps most along the other axis.
    /// Ties go to the one whose centre is closest.
    pub fn focus_neighbour(&self, pane: PaneId, side: Direction) -> Option<PaneId> {
        let geometry = self.geometry(Rect::UNIT);
        let source = geometry.panes.iter().find(|p| p.pane == pane)?.rect;
        let mut best: Option<(PaneId, f64, f64)> = None;
        for candidate in geometry.panes.iter().filter(|p| p.pane != pane) {
            let rect = candidate.rect;
            let (touches, overlap, centre_distance) = match side {
                Direction::Right => (
                    near(rect.x, source.right()),
                    overlap_y(&source, &rect),
                    centre_gap_y(&source, &rect),
                ),
                Direction::Left => (
                    near(rect.right(), source.x),
                    overlap_y(&source, &rect),
                    centre_gap_y(&source, &rect),
                ),
                Direction::Down => (
                    near(rect.y, source.bottom()),
                    overlap_x(&source, &rect),
                    centre_gap_x(&source, &rect),
                ),
                Direction::Up => (
                    near(rect.bottom(), source.y),
                    overlap_x(&source, &rect),
                    centre_gap_x(&source, &rect),
                ),
            };
            if !touches || overlap <= EDGE_EPSILON {
                continue;
            }
            let better = best.is_none_or(|(_, best_overlap, best_distance)| {
                overlap > best_overlap + EDGE_EPSILON
                    || (near(overlap, best_overlap) && centre_distance < best_distance)
            });
            if better {
                best = Some((candidate.pane, overlap, centre_distance));
            }
        }
        best.map(|(id, _, _)| id)
    }
}

fn layout_node(node: &Node, bounds: Rect, out: &mut Geometry) {
    match node {
        Node::Pane { id } => out.panes.push(PaneRect {
            pane: *id,
            rect: bounds,
        }),
        Node::Split { axis, children } => {
            let (start, length) = bounds.span(*axis);
            let mut offset = 0.0;
            for (i, child) in children.iter().enumerate() {
                let child_rect =
                    bounds.with_span(*axis, start + offset * length, child.ratio * length);
                offset += child.ratio;
                if i + 1 < children.len() {
                    out.dividers.push(Divider {
                        anchor: last_leaf(&child.node),
                        axis: *axis,
                        split: bounds,
                        position: offset,
                    });
                }
                layout_node(&child.node, child_rect, out);
            }
        }
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EDGE_EPSILON
}

fn overlap_1d(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

fn overlap_x(a: &Rect, b: &Rect) -> f64 {
    overlap_1d(a.x, a.right(), b.x, b.right())
}

fn overlap_y(a: &Rect, b: &Rect) -> f64 {
    overlap_1d(a.y, a.bottom(), b.y, b.bottom())
}

fn centre_gap_x(a: &Rect, b: &Rect) -> f64 {
    ((a.x + a.width / 2.0) - (b.x + b.width / 2.0)).abs()
}

fn centre_gap_y(a: &Rect, b: &Rect) -> f64 {
    ((a.y + a.height / 2.0) - (b.y + b.height / 2.0)).abs()
}
