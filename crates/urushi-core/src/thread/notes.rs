//! Validation of a thread's notes: free text the user keeps on a thread (UR-64).

use thiserror::Error;

/// Longest notes a thread can have, in characters (the same as a task's).
pub const MAX_THREAD_NOTES: usize = 10_000;

/// Notes longer than [`MAX_THREAD_NOTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("thread notes must be at most {MAX_THREAD_NOTES} characters")]
pub struct NotesTooLong;

/// A thread's notes as stored: trailing whitespace removed, at most
/// [`MAX_THREAD_NOTES`] characters. Blank notes become empty, which means none.
pub fn thread_notes(text: &str) -> Result<String, NotesTooLong> {
    let text = text.trim_end();
    if text.chars().count() > MAX_THREAD_NOTES {
        Err(NotesTooLong)
    } else {
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_lose_trailing_whitespace_and_are_bounded() {
        assert_eq!(
            thread_notes("  keep\nlines  \n\n").unwrap(),
            "  keep\nlines"
        );
        assert_eq!(thread_notes(" \n\t").unwrap(), "");
        assert_eq!(
            thread_notes(&"é".repeat(MAX_THREAD_NOTES))
                .unwrap()
                .chars()
                .count(),
            MAX_THREAD_NOTES
        );
        assert_eq!(
            thread_notes(&"x".repeat(MAX_THREAD_NOTES + 1)),
            Err(NotesTooLong)
        );
    }
}
