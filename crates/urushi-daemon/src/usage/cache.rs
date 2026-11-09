//! When to read an agent's usage again, and what to show in between.
//!
//! Claude's usage endpoint is rate-limited per sign-in, and every running Claude Code
//! shares that budget with us. Claude Code backs off after an HTTP 429 (for `Retry-After`,
//! else 5 minutes, at most an hour); a client that keeps asking every few seconds keeps
//! the budget exhausted and never sees an answer again. So a rate-limited read waits as
//! long as the service asks, doubling the wait while it keeps refusing, and the last good
//! reading stays on show meanwhile. The policy is pure (times are passed in) so it's
//! tested without a clock.

use std::time::{Duration, Instant};

use urushi_core::Timestamp;

use super::Usage;

/// How long a successful reading is reused. Usage moves slowly and clients count reset
/// times down themselves, so this is long; it keeps us well inside a budget shared with
/// every running agent.
pub(super) const USAGE_TTL: Duration = Duration::from_secs(5 * 60);
/// How long a failed reading (other than a rate limit) is reused before trying again.
pub(super) const ERROR_TTL: Duration = Duration::from_secs(30);
/// The wait after a rate limit that doesn't say how long to wait (Claude Code's own).
pub(super) const RATE_LIMIT_WAIT: Duration = Duration::from_secs(5 * 60);
/// The shortest wait after a rate limit, whatever `Retry-After` says.
pub(super) const MIN_RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);
/// The longest wait after a rate limit.
pub(super) const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(60 * 60);

/// Why usage couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Not readable right now: a short, user-facing sentence.
    Unavailable(String),
    /// The usage service refused because it's asked too often (HTTP 429).
    RateLimited {
        /// Who refused, as a user-facing phrase: "Claude's usage service is busy".
        message: String,
        /// How long the service asked us to wait, if it said.
        retry_after: Option<Duration>,
    },
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Unavailable(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::Unavailable(message.to_owned())
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::RateLimited { message, .. } => f.write_str(message),
        }
    }
}

/// The outcome of the latest read.
#[derive(Clone, Debug)]
enum Last {
    Read(Usage),
    Failed(String),
    RateLimited(String),
}

/// What's known about one agent's usage.
#[derive(Clone, Debug)]
pub(super) struct Entry {
    /// When to ask the agent's service again.
    pub(super) next_read: Instant,
    last: Last,
    /// When the latest read happened.
    fetched: Timestamp,
    /// The latest successful reading and when it was read.
    last_good: Option<(Usage, Timestamp)>,
    /// The current rate-limit wait, doubled while the service keeps refusing.
    backoff: Option<Duration>,
}

impl Entry {
    /// What a client is told at `now`: the latest reading, or while rate-limited the last
    /// good one (or how long until the next try, if there's none).
    pub(super) fn shown(&self, now: Instant) -> (Result<Usage, String>, Timestamp) {
        match &self.last {
            Last::Read(usage) => (Ok(usage.clone()), self.fetched),
            Last::Failed(message) => (Err(message.clone()), self.fetched),
            Last::RateLimited(message) => {
                if let Some((usage, at)) = &self.last_good {
                    return (Ok(usage.clone()), *at);
                }
                let wait = self.next_read.saturating_duration_since(now);
                (
                    Err(format!("{message}; trying again {}", in_about(wait))),
                    self.fetched,
                )
            }
        }
    }
}

/// The entry after a read at `now` (`at` on the wall clock) that gave `result`.
pub(super) fn record(
    previous: Option<Entry>,
    result: Result<Usage, Failure>,
    now: Instant,
    at: Timestamp,
) -> Entry {
    let (last_good, previous_backoff) = previous.map_or((None, None), |p| (p.last_good, p.backoff));
    match result {
        Ok(usage) => Entry {
            next_read: now + USAGE_TTL,
            last_good: Some((usage.clone(), at)),
            last: Last::Read(usage),
            fetched: at,
            backoff: None,
        },
        Err(Failure::Unavailable(message)) => Entry {
            next_read: now + ERROR_TTL,
            last: Last::Failed(message),
            fetched: at,
            last_good,
            backoff: None,
        },
        Err(Failure::RateLimited {
            message,
            retry_after,
        }) => {
            let wait = rate_limit_wait(retry_after, previous_backoff);
            Entry {
                next_read: now + wait,
                last: Last::RateLimited(message),
                fetched: at,
                last_good,
                backoff: Some(wait),
            }
        }
    }
}

/// How long to wait after a rate limit: what the service asked for (within bounds), else
/// [`RATE_LIMIT_WAIT`], doubling while it keeps refusing.
pub(super) fn rate_limit_wait(
    retry_after: Option<Duration>,
    previous: Option<Duration>,
) -> Duration {
    match (retry_after, previous) {
        (Some(asked), _) => asked,
        (None, Some(previous)) => previous.saturating_mul(2),
        (None, None) => RATE_LIMIT_WAIT,
    }
    .clamp(MIN_RATE_LIMIT_WAIT, MAX_RATE_LIMIT_WAIT)
}

/// "in 5m", "in 1h 5m", rounded up to the minute.
fn in_about(wait: Duration) -> String {
    let minutes = wait.as_secs().div_ceil(60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("in {m}m"),
        (h, 0) => format!("in {h}h"),
        (h, m) => format!("in {h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    fn usage(percent: f64) -> Usage {
        Usage {
            plan: "Max 5x".into(),
            windows: vec![super::super::Window {
                label: "5h".into(),
                used_percent: percent,
                resets_at_ms: None,
            }],
        }
    }

    fn busy(retry_after: Option<Duration>) -> Result<Usage, Failure> {
        Err(Failure::RateLimited {
            message: "Claude's usage service is busy".into(),
            retry_after,
        })
    }

    #[test]
    fn rate_limit_waits_honour_retry_after_within_bounds_and_double_without_it() {
        let secs = Duration::from_secs;
        assert_eq!(rate_limit_wait(Some(secs(120)), None), secs(120));
        assert_eq!(rate_limit_wait(Some(secs(0)), None), MIN_RATE_LIMIT_WAIT);
        assert_eq!(
            rate_limit_wait(Some(secs(86_400)), None),
            MAX_RATE_LIMIT_WAIT
        );
        assert_eq!(rate_limit_wait(None, None), RATE_LIMIT_WAIT);
        assert_eq!(rate_limit_wait(None, Some(RATE_LIMIT_WAIT)), secs(600));
        assert_eq!(
            rate_limit_wait(None, Some(secs(2_400))),
            MAX_RATE_LIMIT_WAIT
        );
    }

    #[test]
    fn a_rate_limit_keeps_the_last_good_reading_and_waits() {
        let t0 = Instant::now();
        let good = record(
            None,
            Ok(usage(41.0)),
            t0,
            Timestamp::from_unix_millis(1_000),
        );
        assert_eq!(good.next_read, t0 + USAGE_TTL);

        let t1 = t0 + USAGE_TTL;
        let limited = record(
            Some(good),
            busy(None),
            t1,
            Timestamp::from_unix_millis(2_000),
        );
        assert_eq!(
            limited.next_read,
            t1 + RATE_LIMIT_WAIT,
            "not the 30 s error retry"
        );
        let (shown, fetched) = limited.shown(t1);
        assert_eq!(shown, Ok(usage(41.0)));
        assert_eq!(
            fetched,
            Timestamp::from_unix_millis(1_000),
            "when it was really read"
        );

        // Refused again: the wait doubles. Then a success resets it.
        let t2 = limited.next_read;
        let again = record(
            Some(limited),
            busy(None),
            t2,
            Timestamp::from_unix_millis(3_000),
        );
        assert_eq!(again.next_read, t2 + 2 * RATE_LIMIT_WAIT);
        let t3 = again.next_read;
        let fine = record(
            Some(again),
            Ok(usage(50.0)),
            t3,
            Timestamp::from_unix_millis(4_000),
        );
        let t4 = fine.next_read;
        let first = record(
            Some(fine),
            busy(None),
            t4,
            Timestamp::from_unix_millis(5_000),
        );
        assert_eq!(first.next_read, t4 + RATE_LIMIT_WAIT);
        assert_eq!(first.shown(t4).0, Ok(usage(50.0)));
    }

    #[test]
    fn a_rate_limit_without_a_good_reading_says_when_it_tries_again() {
        let t0 = Instant::now();
        let at = Timestamp::from_unix_millis(1_000);
        let limited = record(None, busy(Some(Duration::from_secs(150))), t0, at);
        assert_eq!(
            limited.shown(t0).0,
            Err("Claude's usage service is busy; trying again in 3m".into())
        );
        assert_eq!(
            limited.shown(t0 + Duration::from_secs(140)).0,
            Err("Claude's usage service is busy; trying again in 1m".into())
        );
        let long = record(None, busy(Some(Duration::from_secs(3_900))), t0, at);
        assert_eq!(
            long.shown(t0).0,
            Err("Claude's usage service is busy; trying again in 1h".into()),
            "capped at an hour"
        );
    }

    #[test]
    fn other_failures_are_shown_and_retried_soon() {
        let t0 = Instant::now();
        let at = Timestamp::from_unix_millis(1_000);
        let good = record(None, Ok(usage(10.0)), t0, at);
        let failed = record(Some(good), Err("Claude isn't signed in".into()), t0, at);
        assert_eq!(failed.next_read, t0 + ERROR_TTL);
        assert_eq!(failed.shown(t0).0, Err("Claude isn't signed in".into()));
        assert_eq!(in_about(Duration::from_secs(3_960)), "in 1h 6m");
        assert_eq!(in_about(Duration::ZERO), "in 1m");
    }
}
