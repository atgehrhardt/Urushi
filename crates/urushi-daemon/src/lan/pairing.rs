//! The one pairing code that may be active at a time.
//!
//! A code works once, for [`PAIRING_TTL`], and is withdrawn after [`MAX_FAILURES`] wrong
//! attempts, so it can't be guessed online either. Time is passed in.

use std::time::{Duration, Instant};

use urushi_proto::lan::PairingCode;

/// How long a pairing code works.
pub const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
/// Wrong attempts before a code is withdrawn.
pub const MAX_FAILURES: u32 = 5;

/// An active pairing code.
#[derive(Clone, Copy, Debug)]
pub struct Pairing {
    code: PairingCode,
    expires_at: Instant,
    failures: u32,
}

impl Pairing {
    /// A code that works from `now` for [`PAIRING_TTL`].
    pub fn new(code: PairingCode, now: Instant) -> Self {
        Self {
            code,
            expires_at: now + PAIRING_TTL,
            failures: 0,
        }
    }

    /// The code, unless it has expired or failed too often.
    pub fn code_at(&self, now: Instant) -> Option<PairingCode> {
        (now < self.expires_at && self.failures < MAX_FAILURES).then_some(self.code)
    }

    /// Time left, for showing.
    pub fn remaining_at(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }

    /// Records a wrong attempt.
    pub fn failed(&mut self) {
        self.failures += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_expires() {
        let start = Instant::now();
        let code = PairingCode::generate().unwrap();
        let pairing = Pairing::new(code, start);
        assert_eq!(pairing.code_at(start), Some(code));
        assert_eq!(pairing.remaining_at(start), PAIRING_TTL);
        assert_eq!(pairing.code_at(start + PAIRING_TTL), None);
        assert_eq!(
            pairing.remaining_at(start + PAIRING_TTL * 2),
            Duration::ZERO
        );
    }

    #[test]
    fn a_code_is_withdrawn_after_too_many_failures() {
        let start = Instant::now();
        let mut pairing = Pairing::new(PairingCode::generate().unwrap(), start);
        for _ in 1..MAX_FAILURES {
            pairing.failed();
            assert!(pairing.code_at(start).is_some());
        }
        pairing.failed();
        assert!(pairing.code_at(start).is_none());
    }
}
