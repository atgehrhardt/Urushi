//! Release versions, as Urushi tags them (`vX.Y.Z`) and bundles them (`X.Y.Z`).
//!
//! Updates (ADR 0048) compare the installed version with the latest release. Comparing
//! strings would put `0.1.10` before `0.1.9`, so a version is three numbers. Pre-release
//! and build suffixes are refused: releases are only ever plain `X.Y.Z` tags.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A release version, `major.minor.patch`. Ordered numerically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    /// Incompatible changes.
    pub major: u32,
    /// New features.
    pub minor: u32,
    /// Fixes.
    pub patch: u32,
}

/// Text that isn't a version (`X.Y.Z`, optionally with a leading `v`).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("not a version: {0:?} (expected X.Y.Z)")]
pub struct InvalidVersion(pub String);

impl Version {
    /// A version from its three numbers.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The release tag for this version: `vX.Y.Z`.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

impl FromStr for Version {
    type Err = InvalidVersion;

    /// Parses `X.Y.Z` or `vX.Y.Z`, ignoring surrounding whitespace.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidVersion(s.to_owned());
        let trimmed = s.trim();
        let digits = trimmed.strip_prefix('v').unwrap_or(trimmed);
        let mut parts = digits.split('.');
        let mut number = || -> Result<u32, InvalidVersion> {
            let part = parts.next().ok_or_else(invalid)?;
            // `u32::from_str` accepts a leading `+`; versions don't.
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let version = Self::new(number()?, number()?, number()?);
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(version)
    }
}

impl TryFrom<String> for Version {
    type Error = InvalidVersion;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Version> for String {
    fn from(v: Version) -> Self {
        v.to_string()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_tagged_versions() {
        assert_eq!("0.1.6".parse(), Ok(Version::new(0, 1, 6)));
        assert_eq!("v12.0.3".parse(), Ok(Version::new(12, 0, 3)));
        assert_eq!(" v1.2.3\n".parse(), Ok(Version::new(1, 2, 3)));
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "",
            "v",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1..3",
            "v1.2.3-rc1",
            "+1.2.3",
            "1.+2.3",
            "V1.2.3",
            "99999999999.0.0",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn orders_numerically_not_as_text() {
        let v = |s: &str| s.parse::<Version>().unwrap();
        assert!(v("0.1.10") > v("0.1.9"));
        assert!(v("0.2.0") > v("0.1.99"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("v0.1.6"), v("0.1.6"));
    }

    #[test]
    fn displays_and_tags() {
        let v = Version::new(0, 1, 7);
        assert_eq!(v.to_string(), "0.1.7");
        assert_eq!(v.tag(), "v0.1.7");
    }

    #[test]
    fn serializes_as_a_string() {
        let v = Version::new(1, 2, 3);
        assert_eq!(serde_json::to_string(&v).unwrap(), "\"1.2.3\"");
        assert_eq!(serde_json::from_str::<Version>("\"1.2.3\"").unwrap(), v);
        assert!(serde_json::from_str::<Version>("\"nope\"").is_err());
    }
}
