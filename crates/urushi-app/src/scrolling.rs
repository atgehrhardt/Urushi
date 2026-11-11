//! Scrolling lists and sideways rows (the board's columns, its tabs, its filter chips)
//! from the wheel, a trackpad, a finger, or a drag held near a row's edge. Pure, so the
//! runner's input handling is unit-tested without a window.

use urushi_ui::{Axis, Point, ScrollRegion};

/// How close (logical pixels) to a sideways row's edge a drag must be to scroll it.
pub(crate) const EDGE_ZONE: f32 = 48.0;
/// How far one drag movement at the very edge scrolls the row.
pub(crate) const EDGE_STEP: f32 = 24.0;

/// A wheel or trackpad movement as `(sideways, vertical)` pixels. With Shift held, a
/// mouse wheel that only turns up and down scrolls sideways, as on most systems.
pub(crate) fn wheel_axes(x: f32, y: f32, shift: bool) -> (f32, f32) {
    if shift && x == 0.0 { (y, 0.0) } else { (x, y) }
}

/// `region`'s offset once moved by `pixels` (positive: towards the start, as when the
/// content follows a finger moving down or right), kept within the content.
pub(crate) fn scrolled(region: &ScrollRegion, pixels: f32) -> f32 {
    (region.offset - pixels).clamp(0.0, region.max_offset())
}

/// While dragging at `p` over a sideways `region`: the offset to scroll it to, faster
/// the closer `p` is to an edge, so a card can be carried to a column out of view.
/// `None` when `p` is away from the edges or the row can't go further that way.
pub(crate) fn edge_scrolled(region: &ScrollRegion, p: Point) -> Option<f32> {
    if region.axis != Axis::Horizontal || !region.rect.contains(p) {
        return None;
    }
    let from_left = p.x - region.rect.x;
    let from_right = region.rect.x + region.rect.width - p.x;
    let step = if from_right < EDGE_ZONE {
        EDGE_STEP * (1.0 - from_right / EDGE_ZONE)
    } else if from_left < EDGE_ZONE {
        -EDGE_STEP * (1.0 - from_left / EDGE_ZONE)
    } else {
        return None;
    };
    let offset = (region.offset + step).clamp(0.0, region.max_offset());
    ((offset - region.offset).abs() >= 0.5).then_some(offset)
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "offsets here are exact")]
mod tests {
    use urushi_ui::{Rect, key};

    use super::*;

    fn row(offset: f32) -> ScrollRegion {
        ScrollRegion {
            key: key(&["row"]),
            rect: Rect::new(100.0, 0.0, 400.0, 300.0),
            axis: Axis::Horizontal,
            content_len: 1000.0,
            offset,
        }
    }

    #[test]
    fn shift_turns_a_mouse_wheel_sideways() {
        assert_eq!(wheel_axes(0.0, 30.0, true), (30.0, 0.0));
        assert_eq!(wheel_axes(0.0, 30.0, false), (0.0, 30.0));
        assert_eq!(wheel_axes(12.0, 3.0, true), (12.0, 3.0), "a trackpad's own");
    }

    #[test]
    fn scrolling_stays_within_the_content() {
        assert_eq!(scrolled(&row(100.0), 40.0), 60.0);
        assert_eq!(scrolled(&row(100.0), -40.0), 140.0);
        assert_eq!(scrolled(&row(10.0), 40.0), 0.0);
        assert_eq!(scrolled(&row(590.0), -40.0), 600.0);
    }

    #[test]
    fn a_drag_near_an_edge_scrolls_that_way_until_the_end() {
        // Right at the right edge: a full step on.
        assert_eq!(
            edge_scrolled(&row(100.0), Point::new(499.9, 50.0)).map(f32::round),
            Some(124.0)
        );
        // Halfway into the zone on the left: half a step back.
        assert_eq!(
            edge_scrolled(&row(100.0), Point::new(124.0, 50.0)),
            Some(88.0)
        );
        // In the middle, or with nowhere further to go: nothing.
        assert_eq!(edge_scrolled(&row(100.0), Point::new(300.0, 50.0)), None);
        assert_eq!(edge_scrolled(&row(600.0), Point::new(499.0, 50.0)), None);
        assert_eq!(edge_scrolled(&row(0.0), Point::new(101.0, 50.0)), None);
        // A column doesn't scroll sideways.
        let column = ScrollRegion {
            axis: Axis::Vertical,
            ..row(100.0)
        };
        assert_eq!(edge_scrolled(&column, Point::new(499.0, 50.0)), None);
    }
}
