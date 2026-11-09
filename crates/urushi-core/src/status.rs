//! A thread's status: a short label the user picks (e.g. "In review"), used to group
//! threads in the sidebar. Not to be confused with [`crate::ThreadState`], the lifecycle
//! the daemon manages.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why a status was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StatusError {
    /// Blank after trimming.
    #[error("a status must not be empty")]
    Empty,
    /// Longer than [`Status::MAX_CHARS`].
    #[error("a status must be at most {} characters", Status::MAX_CHARS)]
    TooLong,
}

/// A trimmed, non-empty, single-line label of at most [`Status::MAX_CHARS`] characters.
/// Statuses are user-defined: any label in use on some thread is a status.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Status(String);

impl Status {
    /// Longest allowed label, in characters.
    pub const MAX_CHARS: usize = 40;

    /// Validates a label. Inner whitespace runs (including newlines) become one space.
    pub fn new(label: &str) -> Result<Self, StatusError> {
        let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() {
            Err(StatusError::Empty)
        } else if label.chars().count() > Self::MAX_CHARS {
            Err(StatusError::TooLong)
        } else {
            Ok(Self(label))
        }
    }

    /// The label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Status {
    type Error = StatusError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<Status> for String {
    fn from(value: Status) -> Self {
        value.0
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_trimmed_single_line_and_bounded() {
        assert_eq!(
            Status::new("  In   review\n").unwrap().as_str(),
            "In review"
        );
        assert_eq!(Status::new(" \t"), Err(StatusError::Empty));
        assert_eq!(Status::new(&"x".repeat(41)), Err(StatusError::TooLong));
        assert!(
            Status::new(&"é".repeat(40)).is_ok(),
            "counted in characters"
        );
    }

    #[test]
    fn deserialization_validates() {
        assert!(serde_json::from_str::<Status>("\"  \"").is_err());
        let s: Status = serde_json::from_str("\"Blocked\"").unwrap();
        assert_eq!(s.as_str(), "Blocked");
    }
}
