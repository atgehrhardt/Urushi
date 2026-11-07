//! Logical-pixel geometry. Scale factors are applied only by the renderer.

/// A point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal position, growing to the right.
    pub x: f32,
    /// Vertical position, growing downwards.
    pub y: f32,
}

impl Point {
    /// Builds a point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Size {
    /// Builds a size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Builds a rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Whether `p` is inside (left/top inclusive, right/bottom exclusive).
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Shrinks by `insets` on each side (never below zero size).
    #[must_use]
    pub fn inset(&self, insets: Insets) -> Self {
        Self {
            x: self.x + insets.left,
            y: self.y + insets.top,
            width: (self.width - insets.left - insets.right).max(0.0),
            height: (self.height - insets.top - insets.bottom).max(0.0),
        }
    }

    /// The overlap of two rectangles (zero-sized if they don't overlap).
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }

    /// The centre point.
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Space inside a box's edges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    /// Top.
    pub top: f32,
    /// Right.
    pub right: f32,
    /// Bottom.
    pub bottom: f32,
    /// Left.
    pub left: f32,
}

impl Insets {
    /// The same inset on every side.
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    /// `vertical` on top and bottom, `horizontal` on left and right.
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Total horizontal inset.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Total vertical inset.
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "exact values are expected")]
mod tests {
    use super::*;

    #[test]
    fn rect_ops() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(r.contains(Point::new(10.0, 10.0)));
        assert!(!r.contains(Point::new(110.0, 20.0)));
        assert_eq!(r.inset(Insets::all(5.0)), Rect::new(15.0, 15.0, 90.0, 40.0));
        assert_eq!(r.inset(Insets::all(100.0)).width, 0.0);
        assert_eq!(
            r.intersect(&Rect::new(50.0, 0.0, 100.0, 20.0)),
            Rect::new(50.0, 10.0, 60.0, 10.0)
        );
        assert_eq!(r.intersect(&Rect::new(500.0, 0.0, 1.0, 1.0)).width, 0.0);
    }
}
