//! Timing slow operations step by step, at debug level, so a slow screen can be traced
//! to the step that made it slow (UR-68) instead of guessed at.

use std::time::{Duration, Instant};

/// Logs how long each step of one operation took, and the total so far.
#[derive(Debug)]
pub(crate) struct Stopwatch {
    what: &'static str,
    start: Instant,
    last: Instant,
    /// A step to log when dropped, for operations with many ways out.
    at_drop: Option<&'static str>,
}

impl Stopwatch {
    /// Starts timing `what` (e.g. "create thread").
    pub(crate) fn start(what: &'static str) -> Self {
        let now = Instant::now();
        Self {
            what,
            start: now,
            last: now,
            at_drop: None,
        }
    }

    /// Also logs `step` when dropped, however the operation ends.
    pub(crate) fn on_drop(mut self, step: &'static str) -> Self {
        self.at_drop = Some(step);
        self
    }

    /// Logs the time since the previous step (or the start) as `step`.
    pub(crate) fn lap(&mut self, step: &str) {
        self.lap_with(step, "");
    }

    /// Like [`Stopwatch::lap`], with a word on how the step went (e.g. "reused").
    pub(crate) fn lap_with(&mut self, step: &str, outcome: &str) {
        let now = Instant::now();
        tracing::debug!(
            what = self.what,
            step,
            outcome,
            step_ms = millis(now - self.last),
            total_ms = millis(now - self.start),
            "timing"
        );
        self.last = now;
    }
}

impl Drop for Stopwatch {
    fn drop(&mut self) {
        if let Some(step) = self.at_drop.take() {
            self.lap(step);
        }
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}
