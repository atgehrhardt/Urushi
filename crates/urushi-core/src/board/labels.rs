//! Labels that organize tasks: a color for a task or a column, and free-text tags on a
//! task.
//!
//! Colors are a fixed palette of names, not RGB values: each client picks the shade that
//! suits its theme (light or dark), and a board looks consistent everywhere. A tag's
//! color comes from its name ([`tag_color`]), so the same tag has the same color on
//! every card and every client without being stored.

use serde::{Deserialize, Serialize};

use super::BoardError;

/// Most tags one task can have.
pub const MAX_TAGS: usize = 10;
/// Longest tag, in characters.
pub const MAX_TAG: usize = 32;

/// A color from the board palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelColor {
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Teal.
    Teal,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
    /// Pink.
    Pink,
    /// Gray.
    Gray,
}

impl LabelColor {
    /// Every color, in palette order.
    pub const ALL: [Self; 9] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Gray,
    ];

    /// The lowercase name (`"red"`), as shown and as typed in the CLI.
    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Teal => "teal",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Pink => "pink",
            Self::Gray => "gray",
        }
    }

    /// The color with this name (any case; "grey" too).
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        if name == "grey" {
            return Some(Self::Gray);
        }
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// Position in [`Self::ALL`], for palettes indexed the same way.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// A task's tags, cleaned up: whitespace collapsed, blanks and repeats (ignoring case)
/// dropped, the first spelling kept, in order. Refuses tags over [`MAX_TAG`] characters
/// and more than [`MAX_TAGS`] tags.
pub fn task_tags<S: AsRef<str>>(tags: &[S]) -> Result<Vec<String>, BoardError> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag
            .as_ref()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if tag.is_empty() || out.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            continue;
        }
        if tag.chars().count() > MAX_TAG {
            return Err(BoardError::TooLong {
                what: "a tag",
                max: MAX_TAG,
            });
        }
        out.push(tag);
    }
    if out.len() > MAX_TAGS {
        return Err(BoardError::TooManyTags);
    }
    Ok(out)
}

/// The color a tag is shown in: chosen from its name (ignoring case), so it is the same
/// everywhere. Gray is left for "no color".
pub fn tag_color(tag: &str) -> LabelColor {
    // FNV-1a: tiny, stable across platforms and releases (unlike `DefaultHasher`).
    let mut hash: u32 = 0x811c_9dc5;
    for byte in tag.to_lowercase().bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let colors = &LabelColor::ALL[..LabelColor::ALL.len() - 1];
    colors[hash as usize % colors.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_have_names_that_parse_back() {
        for c in LabelColor::ALL {
            assert_eq!(LabelColor::parse(c.name()), Some(c));
            assert_eq!(LabelColor::ALL[c.index()], c);
        }
        assert_eq!(LabelColor::parse(" Grey "), Some(LabelColor::Gray));
        assert_eq!(LabelColor::parse("mauve"), None);
    }

    #[test]
    fn tags_are_cleaned_up_and_bounded() {
        assert_eq!(
            task_tags(&["  ui ", "Bug  fix", "UI", "", "bug fix"]).unwrap(),
            ["ui", "Bug fix"]
        );
        assert!(matches!(
            task_tags(&["x".repeat(MAX_TAG + 1)]),
            Err(BoardError::TooLong { .. })
        ));
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert_eq!(task_tags(&many), Err(BoardError::TooManyTags));
    }

    #[test]
    fn a_tags_color_depends_only_on_its_name() {
        assert_eq!(tag_color("Backend"), tag_color("backend"));
        assert_ne!(tag_color("backend"), LabelColor::Gray);
        // Different names spread over the palette.
        let colors: std::collections::HashSet<_> = [
            "ui", "backend", "bug", "docs", "infra", "design", "perf", "ios",
        ]
        .iter()
        .map(|t| tag_color(t))
        .collect();
        assert!(colors.len() >= 4, "{colors:?}");
    }
}
