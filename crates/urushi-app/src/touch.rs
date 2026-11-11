//! Touchscreen gestures, turned into the pointer input the rest of the app understands.
//!
//! Phones scroll by dragging a finger, so a finger that moves scrolls whatever is under
//! it. In a terminal the runner turns that into the wheel (scrollback, or the running
//! app's own scrolling). Elsewhere it scrolls lists. A finger that lifts without moving
//! is a tap: a left click where it touched down. Selecting text and dragging things
//! (reordering threads, moving a text caret) need the mouse's press-and-drag, so a
//! finger held still for [`LONG_PRESS`] before it moves acts as the left button. A touch
//! that starts on a split divider grabs it at once, so splits resize with one drag. A
//! finger that sets off sideways is a swipe: it scrolls nothing, and lifting it after
//! [`SWIPE`] reports which way it went (a phone's board changes column, ADR 0046).
//!
//! Only one finger is followed at a time; others are ignored until it lifts. Time comes
//! in as a value, so the gestures are unit-tested without a clock.

use std::time::{Duration, Instant};

use urushi_ui::Point;

/// How far (logical pixels) a finger may wobble before a touch counts as moving.
pub(crate) const SLOP: f32 = 8.0;
/// How long a finger must rest before moving it drags like the mouse instead of
/// scrolling.
pub(crate) const LONG_PRESS: Duration = Duration::from_millis(400);
/// How far (logical pixels) a sideways finger must travel to count as a swipe.
pub(crate) const SWIPE: f32 = 60.0;

/// What the runner should do for a touch event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TouchAction {
    /// Press the left button here.
    Press(Point),
    /// Move with the left button down.
    Drag(Point),
    /// Release the left button here.
    Release(Point),
    /// Scroll what's under `at` by `dy` logical pixels. Positive when the finger moves
    /// down, which reveals what's above (older terminal output).
    Scroll { at: Point, dy: f32 },
    /// A sideways swipe that started at `at` and went `dx` logical pixels (negative:
    /// to the left).
    Swipe { at: Point, dx: f32 },
}

/// What the followed finger is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Down, but hasn't moved past [`SLOP`] yet: still a tap, a scroll or a long press.
    Undecided,
    /// Scrolling.
    Scrolling,
    /// Swiping sideways: reported when it lifts.
    Swiping,
    /// Acting as the left mouse button.
    Pointer,
}

struct Active {
    id: u64,
    start: Point,
    last: Point,
    began: Instant,
    mode: Mode,
}

/// Follows one finger at a time and decides what its gesture means.
#[derive(Default)]
pub(crate) struct Touches {
    active: Option<Active>,
}

impl Touches {
    /// Finger `id` touched down at `p`. `grab` makes it the left button at once (a
    /// divider is under it).
    pub(crate) fn began(
        &mut self,
        id: u64,
        p: Point,
        now: Instant,
        grab: bool,
    ) -> Vec<TouchAction> {
        if self.active.is_some() {
            return Vec::new();
        }
        let mode = if grab { Mode::Pointer } else { Mode::Undecided };
        self.active = Some(Active {
            id,
            start: p,
            last: p,
            began: now,
            mode,
        });
        if grab {
            vec![TouchAction::Press(p)]
        } else {
            Vec::new()
        }
    }

    /// Finger `id` moved to `p`.
    pub(crate) fn moved(&mut self, id: u64, p: Point, now: Instant) -> Vec<TouchAction> {
        let Some(active) = self.active.as_mut().filter(|a| a.id == id) else {
            return Vec::new();
        };
        match active.mode {
            Mode::Undecided => {
                if distance(active.start, p) < SLOP {
                    return Vec::new();
                }
                if now.duration_since(active.began) >= LONG_PRESS {
                    active.mode = Mode::Pointer;
                    active.last = p;
                    vec![TouchAction::Press(active.start), TouchAction::Drag(p)]
                } else if (p.x - active.start.x).abs() > 2.0 * (p.y - active.start.y).abs() {
                    active.mode = Mode::Swiping;
                    active.last = p;
                    Vec::new()
                } else {
                    active.mode = Mode::Scrolling;
                    // Include the slop, so the content stays under the finger.
                    Self::scroll(active, p)
                }
            }
            Mode::Scrolling => Self::scroll(active, p),
            Mode::Swiping => {
                active.last = p;
                Vec::new()
            }
            Mode::Pointer => {
                active.last = p;
                vec![TouchAction::Drag(p)]
            }
        }
    }

    /// Finger `id` lifted at `p` (or the system took it over: `cancelled`).
    pub(crate) fn ended(&mut self, id: u64, p: Point, cancelled: bool) -> Vec<TouchAction> {
        if self.active.as_ref().is_none_or(|a| a.id != id) {
            return Vec::new();
        }
        let Some(active) = self.active.take() else {
            return Vec::new();
        };
        match active.mode {
            Mode::Undecided if !cancelled => vec![
                TouchAction::Press(active.start),
                TouchAction::Release(active.start),
            ],
            Mode::Swiping if !cancelled && (p.x - active.start.x).abs() >= SWIPE => {
                vec![TouchAction::Swipe {
                    at: active.start,
                    dx: p.x - active.start.x,
                }]
            }
            Mode::Undecided | Mode::Scrolling | Mode::Swiping => Vec::new(),
            // The lift can arrive without a move to where it happened (Android batches a
            // last move with the lift), so drag there first: a selection ends under the
            // finger.
            Mode::Pointer if p != active.last => {
                vec![TouchAction::Drag(p), TouchAction::Release(p)]
            }
            Mode::Pointer => vec![TouchAction::Release(p)],
        }
    }

    /// Scrolls by the finger's vertical movement since the last event. The gesture keeps
    /// scrolling what was under it at the start, wherever the finger goes.
    fn scroll(active: &mut Active, p: Point) -> Vec<TouchAction> {
        let dy = p.y - active.last.y;
        active.last = p;
        if dy == 0.0 {
            Vec::new()
        } else {
            vec![TouchAction::Scroll {
                at: active.start,
                dy,
            }]
        }
    }
}

/// Where a gesture's actions were a tap (a press and release in one spot, nothing held
/// or dragged), if they were.
pub(crate) fn tap_at(actions: &[TouchAction]) -> Option<Point> {
    match actions {
        [TouchAction::Press(a), TouchAction::Release(b)] if a == b => Some(*b),
        _ => None,
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn a_finger_that_moves_scrolls_and_never_presses() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        assert!(touches.began(1, at(100.0, 100.0), t0, false).is_empty());
        let t = t0 + Duration::from_millis(50);
        assert_eq!(
            touches.moved(1, at(101.0, 130.0), t),
            vec![TouchAction::Scroll {
                at: at(100.0, 100.0),
                dy: 30.0
            }]
        );
        assert_eq!(
            touches.moved(1, at(101.0, 110.0), t),
            vec![TouchAction::Scroll {
                at: at(100.0, 100.0),
                dy: -20.0
            }]
        );
        assert!(touches.ended(1, at(101.0, 110.0), false).is_empty());
    }

    #[test]
    fn a_finger_that_sets_off_sideways_swipes_when_it_lifts() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(300.0, 400.0), t0, false);
        let t = t0 + Duration::from_millis(50);
        assert!(
            touches.moved(1, at(270.0, 405.0), t).is_empty(),
            "no scroll"
        );
        assert!(touches.moved(1, at(200.0, 430.0), t).is_empty());
        assert_eq!(
            touches.ended(1, at(180.0, 430.0), false),
            vec![TouchAction::Swipe {
                at: at(300.0, 400.0),
                dx: -120.0
            }]
        );
        // Too short, or taken over by the system: nothing.
        touches.began(1, at(100.0, 100.0), t0, false);
        touches.moved(1, at(130.0, 100.0), t);
        assert!(touches.ended(1, at(140.0, 100.0), false).is_empty());
        touches.began(1, at(100.0, 100.0), t0, false);
        touches.moved(1, at(200.0, 100.0), t);
        assert!(touches.ended(1, at(200.0, 100.0), true).is_empty());
        // Mostly downwards still scrolls.
        touches.began(1, at(100.0, 100.0), t0, false);
        assert_eq!(
            touches.moved(1, at(115.0, 130.0), t),
            vec![TouchAction::Scroll {
                at: at(100.0, 100.0),
                dy: 30.0
            }]
        );
    }

    #[test]
    fn wobbling_within_the_slop_is_still_a_tap() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(50.0, 50.0), t0, false);
        assert!(touches.moved(1, at(53.0, 54.0), t0).is_empty());
        assert_eq!(
            touches.ended(1, at(53.0, 54.0), false),
            vec![
                TouchAction::Press(at(50.0, 50.0)),
                TouchAction::Release(at(50.0, 50.0))
            ]
        );
    }

    #[test]
    fn holding_still_then_moving_drags_like_the_mouse() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(10.0, 10.0), t0, false);
        let later = t0 + LONG_PRESS;
        assert_eq!(
            touches.moved(1, at(40.0, 10.0), later),
            vec![
                TouchAction::Press(at(10.0, 10.0)),
                TouchAction::Drag(at(40.0, 10.0))
            ]
        );
        assert_eq!(
            touches.moved(1, at(60.0, 12.0), later),
            vec![TouchAction::Drag(at(60.0, 12.0))]
        );
        assert_eq!(
            touches.ended(1, at(60.0, 12.0), false),
            vec![TouchAction::Release(at(60.0, 12.0))]
        );
    }

    #[test]
    fn a_drag_ends_where_the_finger_lifts() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(10.0, 10.0), t0, false);
        touches.moved(1, at(40.0, 10.0), t0 + LONG_PRESS);
        assert_eq!(
            touches.ended(1, at(90.0, 30.0), false),
            vec![
                TouchAction::Drag(at(90.0, 30.0)),
                TouchAction::Release(at(90.0, 30.0))
            ]
        );
    }

    #[test]
    fn a_grabbed_touch_presses_at_once() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        assert_eq!(
            touches.began(1, at(5.0, 5.0), t0, true),
            vec![TouchAction::Press(at(5.0, 5.0))]
        );
        assert_eq!(
            touches.moved(1, at(30.0, 5.0), t0),
            vec![TouchAction::Drag(at(30.0, 5.0))]
        );
    }

    #[test]
    fn a_second_finger_is_ignored_until_the_first_lifts() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(0.0, 0.0), t0, false);
        assert!(touches.began(2, at(50.0, 50.0), t0, false).is_empty());
        assert!(touches.moved(2, at(50.0, 90.0), t0).is_empty());
        assert!(touches.ended(2, at(50.0, 90.0), false).is_empty());
        touches.ended(1, at(0.0, 0.0), false);
        assert!(touches.began(2, at(50.0, 50.0), t0, false).is_empty());
    }

    #[test]
    fn a_cancelled_touch_does_nothing() {
        let t0 = Instant::now();
        let mut touches = Touches::default();
        touches.began(1, at(0.0, 0.0), t0, false);
        assert!(touches.ended(1, at(0.0, 0.0), true).is_empty());
    }

    #[test]
    fn only_a_press_and_release_in_one_spot_is_a_tap() {
        let p = at(5.0, 5.0);
        assert_eq!(
            tap_at(&[TouchAction::Press(p), TouchAction::Release(p)]),
            Some(p)
        );
        let held = [TouchAction::Release(p)];
        assert_eq!(tap_at(&held), None, "a long press");
        let dragged = [
            TouchAction::Drag(at(9.0, 5.0)),
            TouchAction::Release(at(9.0, 5.0)),
        ];
        assert_eq!(tap_at(&dragged), None);
    }
}
