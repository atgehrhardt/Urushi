//! Wall-clock timestamps as plain values.
//!
//! This crate never reads the clock. The daemon reads it and passes `Timestamp`s in.

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (UTC).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    /// The epoch, standing in for a time that was never recorded.
    pub fn unknown() -> Self {
        Self(0)
    }

    /// Creates a timestamp from milliseconds since the Unix epoch.
    pub const fn from_unix_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Milliseconds since the Unix epoch.
    pub const fn unix_millis(self) -> u64 {
        self.0
    }
}
