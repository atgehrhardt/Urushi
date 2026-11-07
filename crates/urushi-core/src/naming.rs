//! Human-readable names that are also valid machine identifiers: slugs and git branch names.
//!
//! Branch names follow the rules of `git check-ref-format` (for names under `refs/heads/`),
//! so a [`BranchName`] can always be passed to `git` without further checks.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ThreadId;

/// Errors from validating names.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamingError {
    /// The name is not a valid git branch name.
    #[error("invalid branch name {name:?}: {reason}")]
    InvalidBranchName {
        /// The rejected name.
        name: String,
        /// Which rule it broke.
        reason: &'static str,
    },
    /// The prefix would not produce valid branch names.
    #[error("invalid branch prefix {prefix:?}: {reason}")]
    InvalidBranchPrefix {
        /// The rejected prefix.
        prefix: String,
        /// Which rule it broke.
        reason: &'static str,
    },
}

/// A lowercase `[a-z0-9-]` identifier made from a human title, e.g. "Fix login bug!" → `fix-login-bug`.
///
/// Always non-empty and never starts or ends with `-`, so it is safe to use as a path
/// component or inside a branch name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Slug(String);

impl Slug {
    /// Maximum length in bytes.
    pub const MAX_LEN: usize = 40;
    /// Used when the title has no usable characters.
    pub const FALLBACK: &'static str = "thread";

    /// Turns any title into a slug. Never fails.
    pub fn from_title(title: &str) -> Self {
        let mut out = String::with_capacity(title.len().min(Self::MAX_LEN));
        let mut pending_dash = false;
        for ch in title.chars() {
            if ch.is_ascii_alphanumeric() {
                if pending_dash && !out.is_empty() {
                    out.push('-');
                }
                pending_dash = false;
                out.push(ch.to_ascii_lowercase());
            } else {
                pending_dash = true;
            }
            if out.len() >= Self::MAX_LEN {
                break;
            }
        }
        out.truncate(Self::MAX_LEN);
        let trimmed = out.trim_end_matches('-');
        if trimmed.is_empty() {
            Self(Self::FALLBACK.to_owned())
        } else {
            Self(trimmed.to_owned())
        }
    }

    /// The slug text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated git branch name (the part after `refs/heads/`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BranchName(String);

impl BranchName {
    /// Validates `name` against git's ref-name rules.
    pub fn new(name: impl Into<String>) -> Result<Self, NamingError> {
        let name = name.into();
        match ref_name_violation(&name) {
            None => Ok(Self(name)),
            Some(reason) => Err(NamingError::InvalidBranchName { name, reason }),
        }
    }

    /// The primary branch for a new thread: `<prefix><slug>-<short id>`.
    pub fn for_thread(
        prefix: &BranchPrefix,
        slug: &Slug,
        thread: ThreadId,
    ) -> Result<Self, NamingError> {
        Self::new(format!(
            "{}{}-{}",
            prefix.as_str(),
            slug.as_str(),
            thread.short()
        ))
    }

    /// The branch name text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BranchName {
    type Error = NamingError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BranchName> for String {
    fn from(value: BranchName) -> Self {
        value.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A prefix for every branch Urushi creates (default `urushi/`), so they are easy to spot
/// and group. May be empty.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BranchPrefix(String);

impl BranchPrefix {
    /// The default prefix.
    pub const DEFAULT: &'static str = "urushi/";

    /// Validates that `prefix` followed by a slug forms a valid branch name.
    pub fn new(prefix: impl Into<String>) -> Result<Self, NamingError> {
        let prefix = prefix.into();
        if prefix.is_empty() {
            return Ok(Self(prefix));
        }
        // A slug always starts with an alphanumeric character, so checking with one
        // representative character covers every possible suffix.
        match ref_name_violation(&format!("{prefix}x")) {
            None => Ok(Self(prefix)),
            Some(reason) => Err(NamingError::InvalidBranchPrefix { prefix, reason }),
        }
    }

    /// The prefix text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for BranchPrefix {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl TryFrom<String> for BranchPrefix {
    type Error = NamingError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BranchPrefix> for String {
    fn from(value: BranchPrefix) -> Self {
        value.0
    }
}

/// Returns the first `git check-ref-format` rule that `name` breaks, or `None` if it is valid.
fn ref_name_violation(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("must not be empty");
    }
    if name == "@" {
        return Some("must not be \"@\"");
    }
    if name.starts_with('-') {
        return Some("must not start with '-'");
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Some("must not start or end with '/'");
    }
    if name.ends_with('.') {
        return Some("must not end with '.'");
    }
    if name.contains("..") {
        return Some("must not contain \"..\"");
    }
    if name.contains("//") {
        return Some("must not contain \"//\"");
    }
    if name.contains("@{") {
        return Some("must not contain \"@{\"");
    }
    if name.chars().any(|c| {
        c.is_ascii_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return Some("must not contain spaces, control characters, or any of ~^:?*[\\");
    }
    for component in name.split('/') {
        if component.starts_with('.') {
            return Some("path components must not start with '.'");
        }
        #[allow(
            clippy::case_sensitive_file_extension_comparisons,
            reason = "git's rule is case-sensitive: only a literal \".lock\" suffix is reserved"
        )]
        if component.ends_with(".lock") {
            return Some("path components must not end with \".lock\"");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_title() {
        assert_eq!(Slug::from_title("Fix login bug!").as_str(), "fix-login-bug");
        assert_eq!(
            Slug::from_title("  --Hello__World--  ").as_str(),
            "hello-world"
        );
        assert_eq!(Slug::from_title("Ünïcode only ✨").as_str(), "n-code-only");
        assert_eq!(Slug::from_title("!!!").as_str(), Slug::FALLBACK);
        assert_eq!(Slug::from_title("").as_str(), Slug::FALLBACK);
    }

    #[test]
    fn slug_is_bounded_and_never_ends_with_dash() {
        let slug = Slug::from_title(&"ab ".repeat(100));
        assert!(slug.as_str().len() <= Slug::MAX_LEN);
        assert!(!slug.as_str().ends_with('-'));
    }

    #[test]
    fn branch_name_rules() {
        for ok in ["main", "urushi/fix-1", "feature/a.b", "a@b"] {
            assert!(BranchName::new(ok).is_ok(), "{ok} should be valid");
        }
        for bad in [
            "",
            "@",
            "-x",
            "/x",
            "x/",
            "x.",
            "a..b",
            "a//b",
            "a@{b",
            "a b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            ".x",
            "a/.x",
            "x.lock",
            "a/x.lock/b",
            "a\tb",
        ] {
            assert!(BranchName::new(bad).is_err(), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn branch_for_thread() {
        let thread = ThreadId::from_u128(0xdead_beef);
        let name = BranchName::for_thread(
            &BranchPrefix::default(),
            &Slug::from_title("Add OAuth"),
            thread,
        )
        .unwrap();
        assert_eq!(name.as_str(), "urushi/add-oauth-deadbeef");
    }

    #[test]
    fn branch_prefix_rules() {
        assert!(BranchPrefix::new("").is_ok());
        assert!(BranchPrefix::new("agents/").is_ok());
        assert!(BranchPrefix::new("u-").is_ok());
        assert!(BranchPrefix::new("/bad").is_err());
        assert!(BranchPrefix::new("bad//").is_err());
        assert!(BranchPrefix::new("has space/").is_err());
    }

    #[test]
    fn branch_name_deserialization_validates() {
        let bad: Result<BranchName, _> = serde_json::from_str("\"a..b\"");
        assert!(bad.is_err());
        let ok: BranchName = serde_json::from_str("\"urushi/x\"").unwrap();
        assert_eq!(ok.as_str(), "urushi/x");
    }
}
