//! The running indicators' pulse (ADR 0033): how bright they are at a given moment, and
//! when the runner should draw the next frame. It draws a few frames a second, and only
//! while an agent is running, so an idle window stays idle.

use std::time::{Duration, Instant};

/// One full pulse, bright to faint and back.
pub(crate) const PERIOD: Duration = Duration::from_millis(1600);
/// Time between two frames of the pulse (about 12 a second: smooth enough for a small
/// dot, and cheap).
pub(crate) const FRAME: Duration = Duration::from_millis(83);

/// The pulse's clock.
#[derive(Debug)]
pub(crate) struct Pulse {
    /// When the pulse started; brightness is a function of the time since.
    started: Instant,
    /// When the last frame of the pulse was drawn.
    drawn: Option<Instant>,
}

impl Pulse {
    /// A pulse starting at `now`.
    pub(crate) const fn new(now: Instant) -> Self {
        Self {
            started: now,
            drawn: None,
        }
    }

    /// The brightness at `now`, 0 to 1: full at the start of each period, faintest
    /// halfway through.
    pub(crate) fn brightness(&self, now: Instant) -> f32 {
        let t = now.duration_since(self.started).as_secs_f32() / PERIOD.as_secs_f32();
        0.5 + 0.5 * (t * std::f32::consts::TAU).cos()
    }

    /// Notes that a frame was drawn at `now`.
    pub(crate) fn drew(&mut self, now: Instant) {
        self.drawn = Some(now);
    }

    /// When the next frame is due: `now` if one is due already.
    pub(crate) fn next_frame(&self, now: Instant) -> Instant {
        self.drawn.map_or(now, |at| (at + FRAME).max(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_goes_from_full_to_faint_and_back() {
        let start = Instant::now();
        let pulse = Pulse::new(start);
        assert!((pulse.brightness(start) - 1.0).abs() < 1e-4);
        assert!(pulse.brightness(start + PERIOD / 2) < 1e-4);
        assert!((pulse.brightness(start + PERIOD) - 1.0).abs() < 1e-3);
        let quarter = pulse.brightness(start + PERIOD / 4);
        assert!((quarter - 0.5).abs() < 1e-3, "{quarter}");
    }

    #[test]
    fn frames_are_spaced_out() {
        let start = Instant::now();
        let mut pulse = Pulse::new(start);
        assert_eq!(pulse.next_frame(start), start, "the first is due at once");
        pulse.drew(start);
        assert_eq!(pulse.next_frame(start), start + FRAME);
        let late = start + FRAME * 3;
        assert_eq!(pulse.next_frame(late), late, "a late frame is due now");
    }
}
