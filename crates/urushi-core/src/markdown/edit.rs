//! What a formatting control does to Markdown text: wrap the selection in marks, put a
//! list or heading marker on the selected lines, make a link, or continue a list on
//! Enter. Each function returns an [`Edit`] instead of changing the text, so an editor
//! applies it the same way it applies typing (and can undo it).
//!
//! Selections are byte ranges with `start <= end`, on character boundaries. An empty
//! one is the caret.

use std::ops::Range;

use super::block::{Block, BlockKind};

#[cfg(test)]
mod tests;

/// One replacement, and the selection to show afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// The bytes replaced, in the text before the edit.
    pub range: Range<usize>,
    /// What replaces them.
    pub insert: String,
    /// The selection afterwards as `(anchor, caret)`, in the text after the edit.
    pub selection: (usize, usize),
}

impl Edit {
    /// `text` with the edit made.
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + self.insert.len());
        out.push_str(&text[..self.range.start]);
        out.push_str(&self.insert);
        out.push_str(&text[self.range.end..]);
        out
    }
}

/// A line marker the toolbar puts on lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    /// `## `.
    Heading,
    /// `- `.
    Bullet,
    /// `1. `, `2. `, … down the lines.
    Numbered,
    /// `- [ ] `.
    Task,
}

impl Prefix {
    fn matches(self, kind: BlockKind) -> bool {
        matches!(
            (self, kind),
            (Self::Heading, BlockKind::Heading(_))
                | (Self::Bullet, BlockKind::Bullet)
                | (Self::Numbered, BlockKind::Numbered(_))
                | (Self::Task, BlockKind::Task { .. })
        )
    }

    fn marker(self, number: u64) -> String {
        match self {
            Self::Heading => "## ".to_owned(),
            Self::Bullet => "- ".to_owned(),
            Self::Numbered => format!("{number}. "),
            Self::Task => "- [ ] ".to_owned(),
        }
    }
}

/// Wraps the selection in `delim` (`**`, `_`, `~~`, `` ` ``), or unwraps it when it
/// already is wrapped, whether or not the selection includes the marks. Marks hug the
/// text, so spaces at the ends of the selection stay outside them. With no selection it
/// inserts a pair and puts the caret between.
pub fn toggle_wrap(text: &str, sel: Range<usize>, delim: &str) -> Edit {
    let d = delim.len();
    let inner = &text[sel.clone()];
    if inner.len() >= 2 * d && inner.starts_with(delim) && inner.ends_with(delim) {
        let bare = &inner[d..inner.len() - d];
        return Edit {
            range: sel.clone(),
            insert: bare.to_owned(),
            selection: (sel.start, sel.start + bare.len()),
        };
    }
    if text[..sel.start].ends_with(delim) && text[sel.end..].starts_with(delim) {
        return Edit {
            range: sel.start - d..sel.end + d,
            insert: inner.to_owned(),
            selection: (sel.start - d, sel.end - d),
        };
    }
    if inner.trim().is_empty() {
        return Edit {
            range: sel.end..sel.end,
            insert: delim.repeat(2),
            selection: (sel.end + d, sel.end + d),
        };
    }
    let lead = inner.len() - inner.trim_start().len();
    let trail = inner.len() - inner.trim_end().len();
    let core = sel.start + lead..sel.end - trail;
    Edit {
        insert: format!("{delim}{}{delim}", &text[core.clone()]),
        selection: (core.start + d, core.end + d),
        range: core,
    }
}

/// Puts `prefix`'s marker on every line the selection touches, replacing any list or
/// heading marker they had. When every non-blank line already has it, it removes it
/// instead. Blank lines among several are left alone. The selection stays on the same
/// text.
pub fn toggle_prefix(text: &str, sel: Range<usize>, prefix: Prefix) -> Edit {
    let start = line_start(text, sel.start);
    // A selection that ends at the start of a line doesn't take that line.
    let last = if sel.end > sel.start && text[..sel.end].ends_with('\n') {
        sel.end - 1
    } else {
        sel.end
    };
    let end = line_end(text, last.max(start));
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let blocks: Vec<Block> = lines.iter().map(|l| Block::of(l)).collect();
    let filled = || {
        lines
            .iter()
            .zip(&blocks)
            .filter(|(l, _)| !l.trim().is_empty())
    };
    let remove = filled().count() > 0 && filled().all(|(_, b)| prefix.matches(b.kind));
    let mut out = String::new();
    // Per line: its old start, and where its text started before and starts after.
    let mut map = Vec::with_capacity(lines.len());
    let mut old = start;
    let mut number = 0;
    for (i, (line, block)) in lines.iter().zip(&blocks).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let new = out.len();
        let content = &line[block.content_start()..];
        let indent = &line[..block.indent()];
        if line.trim().is_empty() && lines.len() > 1 {
            out.push_str(line);
        } else {
            // Headings only work unindented.
            if remove || prefix != Prefix::Heading {
                out.push_str(indent);
            }
            if !remove {
                number += 1;
                out.push_str(&prefix.marker(number));
            }
            out.push_str(content);
        }
        let new_content = out.len() - content.len().min(out.len() - new);
        let old_content = old + block.content_start();
        map.push((old, (old_content, start + new_content)));
        old += line.len() + 1;
    }
    // A place in a line's text keeps its place; one in a marker goes to the text's start.
    let moved = |pos: usize| {
        let (oc, nc) = map
            .iter()
            .rev()
            .find(|(o, _)| *o <= pos)
            .map_or((start, start), |(_, m)| *m);
        nc + pos.saturating_sub(oc)
    };
    Edit {
        range: start..end,
        selection: (moved(sel.start), moved(sel.end)),
        insert: out,
    }
}

/// Makes the selection a link. A selected address becomes the link's address, with the
/// caret in its empty label; other text becomes the label, with `url` selected to type
/// over. With no selection it inserts `[text](url)` with `text` selected.
pub fn link(text: &str, sel: Range<usize>) -> Edit {
    let inner = &text[sel.clone()];
    let at = sel.start;
    if (inner.starts_with("https://") || inner.starts_with("http://"))
        && !inner.contains(char::is_whitespace)
    {
        return Edit {
            insert: format!("[]({inner})"),
            selection: (at + 1, at + 1),
            range: sel,
        };
    }
    let label = if inner.is_empty() { "text" } else { inner };
    let insert = format!("[{label}](url)");
    let selection = if inner.is_empty() {
        (at + 1, at + 1 + label.len())
    } else {
        let url = at + label.len() + 3;
        (url, url + 3)
    };
    Edit {
        range: sel,
        insert,
        selection,
    }
}

/// What Enter does at `caret` on a list item or quote: start the next item, or end the
/// list when the item is empty. `None` where Enter just adds a line (elsewhere, or with
/// the caret in the marker).
pub fn continue_list(text: &str, caret: usize) -> Option<Edit> {
    let ls = line_start(text, caret);
    let le = line_end(text, caret);
    let line = &text[ls..le];
    let block = Block::of(line);
    if !block.kind.continues() || caret - ls < block.content_start() {
        return None;
    }
    if line[block.content_start()..].trim().is_empty() {
        return Some(Edit {
            range: ls..le,
            insert: String::new(),
            selection: (ls, ls),
        });
    }
    let bullet = &line[block.marker.start..=block.marker.start];
    let marker = match block.kind {
        BlockKind::Bullet => format!("{bullet} "),
        BlockKind::Numbered(n) => format!("{}. ", n.saturating_add(1)),
        BlockKind::Task { .. } => format!("{bullet} [ ] "),
        _ => "> ".to_owned(),
    };
    let insert = format!("\n{}{marker}", &line[..block.indent()]);
    let after = caret + insert.len();
    Some(Edit {
        range: caret..caret,
        insert,
        selection: (after, after),
    })
}

/// Where the line holding byte `at` starts.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// Where the line holding byte `at` ends (before its `\n`).
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}
