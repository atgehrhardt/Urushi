//! What kind of block one line of Markdown starts: a heading, a list item, a task, a
//! quote, or plain text. It covers the same line markers that Jira descriptions use
//! (the daemon's ADF converter). Fenced code blocks and rules read as plain text,
//! because notes are formatted one line at a time.

use std::ops::Range;

/// Most digits a numbered item's number may have.
const MAX_DIGITS: usize = 9;

/// The kind of line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// Plain text.
    Paragraph,
    /// `#` to `######`: the level, 1 to 6.
    Heading(u8),
    /// `- `, `* ` or `+ `.
    Bullet,
    /// `1. `: the item's number.
    Numbered(u64),
    /// `- [ ] ` or `- [x] `: whether it is done.
    Task {
        /// Checked (`[x]` or `[X]`).
        done: bool,
    },
    /// `> `.
    Quote,
}

impl BlockKind {
    /// Whether Enter on such a line continues it (a list item or a quote).
    pub fn continues(self) -> bool {
        matches!(
            self,
            Self::Bullet | Self::Numbered(_) | Self::Task { .. } | Self::Quote
        )
    }
}

/// One line's block: its kind and where its marker sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// What the line is.
    pub kind: BlockKind,
    /// The marker's bytes, from after the indentation through the space that ends it
    /// (`- [ ] `). Empty for a paragraph.
    pub marker: Range<usize>,
}

impl Block {
    /// The block that `line` starts. `line` must not contain `\n`.
    pub fn of(line: &str) -> Self {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        let block = |kind, len: usize| Self {
            kind,
            marker: indent..indent + len,
        };
        if indent == 0
            && let Some((level, len)) = heading(rest)
        {
            return block(BlockKind::Heading(level), len);
        }
        if let Some(len) = quote(rest) {
            return block(BlockKind::Quote, len);
        }
        if let Some(bullet) = ["- ", "* ", "+ "].iter().find(|m| rest.starts_with(**m)) {
            let after = &rest[bullet.len()..];
            if let Some((done, len)) = task(after) {
                return block(BlockKind::Task { done }, bullet.len() + len);
            }
            return block(BlockKind::Bullet, bullet.len());
        }
        if let Some((number, len)) = numbered(rest) {
            return block(BlockKind::Numbered(number), len);
        }
        Self {
            kind: BlockKind::Paragraph,
            marker: indent..indent,
        }
    }

    /// Where the line's text starts, after its indentation and marker.
    pub fn content_start(&self) -> usize {
        self.marker.end
    }

    /// The indentation before the marker, in bytes.
    pub fn indent(&self) -> usize {
        self.marker.start
    }
}

/// `## ` → (2, 3).
fn heading(rest: &str) -> Option<(u8, usize)> {
    let hashes = rest.bytes().take_while(|b| *b == b'#').count();
    let level = u8::try_from(hashes).ok().filter(|l| (1..=6).contains(l))?;
    let after = &rest[hashes..];
    (after.starts_with(' ')).then_some((level, hashes + 1))
}

/// `> ` (or a bare `>`) → its length.
fn quote(rest: &str) -> Option<usize> {
    let after = rest.strip_prefix('>')?;
    Some(if after.starts_with(' ') { 2 } else { 1 })
}

/// `[ ] ` / `[x] ` after a bullet, or the box alone at the end of the line.
fn task(after: &str) -> Option<(bool, usize)> {
    let done = match after.get(..3)? {
        "[ ]" => false,
        "[x]" | "[X]" => true,
        _ => return None,
    };
    let rest = &after[3..];
    if rest.is_empty() {
        Some((done, 3))
    } else {
        rest.starts_with(' ').then_some((done, 4))
    }
}

/// `12. ` → (12, 4).
fn numbered(rest: &str) -> Option<(u64, usize)> {
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > MAX_DIGITS {
        return None;
    }
    rest[digits..].starts_with(". ").then_some(())?;
    Some((rest[..digits].parse().ok()?, digits + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(line: &str) -> BlockKind {
        Block::of(line).kind
    }

    #[test]
    fn markers_are_recognised() {
        assert_eq!(kind("## Next"), BlockKind::Heading(2));
        assert_eq!(kind("###### six"), BlockKind::Heading(6));
        assert_eq!(kind("- one"), BlockKind::Bullet);
        assert_eq!(kind("* one"), BlockKind::Bullet);
        assert_eq!(kind("+ one"), BlockKind::Bullet);
        assert_eq!(kind("12. twelve"), BlockKind::Numbered(12));
        assert_eq!(kind("- [ ] todo"), BlockKind::Task { done: false });
        assert_eq!(kind("- [X] done"), BlockKind::Task { done: true });
        assert_eq!(kind("- [ ]"), BlockKind::Task { done: false });
        assert_eq!(kind("> quoted"), BlockKind::Quote);
        assert_eq!(kind(">quoted"), BlockKind::Quote);
    }

    #[test]
    fn what_isnt_a_marker_is_a_paragraph() {
        for line in [
            "",
            "plain",
            "#no space",
            "####### seven",
            "  ## indented heading",
            "**bold** start",
            "-dash",
            "---",
            "1.5 litres",
            "1234567890. too long",
            "- [y] not a box",
        ] {
            let block = Block::of(line);
            assert!(
                matches!(block.kind, BlockKind::Paragraph | BlockKind::Bullet),
                "{line:?}"
            );
            if block.kind == BlockKind::Paragraph {
                assert!(block.marker.is_empty(), "{line:?}");
            }
        }
        assert_eq!(kind("- [y] not a box"), BlockKind::Bullet);
    }

    #[test]
    fn the_marker_covers_its_space_and_skips_indentation() {
        let b = Block::of("  - [x] done");
        assert_eq!(b.marker, 2..8);
        assert_eq!(b.indent(), 2);
        assert_eq!(&"  - [x] done"[b.content_start()..], "done");
        assert_eq!(Block::of("3. three").marker, 0..3);
        assert_eq!(Block::of("# H").marker, 0..2);
        assert_eq!(Block::of("plain").content_start(), 0);
        assert_eq!(Block::of("   plain").content_start(), 3);
    }

    #[test]
    fn list_items_and_quotes_continue_and_the_rest_dont() {
        assert!(BlockKind::Bullet.continues());
        assert!(BlockKind::Numbered(1).continues());
        assert!(BlockKind::Task { done: true }.continues());
        assert!(BlockKind::Quote.continues());
        assert!(!BlockKind::Heading(1).continues());
        assert!(!BlockKind::Paragraph.continues());
    }
}
