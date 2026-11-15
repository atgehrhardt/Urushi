//! The Markdown that thread notes are written in (UR-72, ADR 0058), read one line at a
//! time: each line's block (heading, list item, task, quote) and its inline marks, as
//! byte ranges of the source. Clients draw notes from this: an editor shows the source
//! with the markup dimmed, and a preview leaves the markup out. [`edit`] has the edits
//! behind the formatting controls.
//!
//! Pure and allocation-light. The daemon stores notes as the text the user typed and
//! never parses them.

pub mod block;
pub mod edit;
pub mod inline;

pub use block::{Block, BlockKind};
pub use edit::{Edit, Prefix, continue_list, link, toggle_prefix, toggle_wrap};
pub use inline::{Span, Style, spans};

/// One line of Markdown: its block, and spans covering every byte of it, with the
/// block's marker (and the indentation before it) as a marker span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What the line is.
    pub block: Block,
    /// Its runs, in order, with ranges into the line.
    pub spans: Vec<Span>,
}

/// Parses one line (without its `\n`).
pub fn line(text: &str) -> Line {
    let block = Block::of(text);
    let start = block.content_start();
    let mut out = Vec::new();
    if start > 0 {
        out.push(Span {
            range: 0..start,
            style: Style::default(),
            marker: true,
        });
    }
    out.extend(spans(&text[start..], start));
    Line { block, spans: out }
}

/// Every line of `text` with the byte where it starts, `\n`s left out.
pub fn lines(text: &str) -> impl Iterator<Item = (usize, Line)> + '_ {
    let mut at = 0;
    text.split('\n').map(move |l| {
        let start = at;
        at += l.len() + 1;
        (start, line(l))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_its_marker_then_its_inline_spans() {
        let l = line("- [ ] **do** it");
        assert_eq!(l.block.kind, BlockKind::Task { done: false });
        assert_eq!(l.spans[0].range, 0..6);
        assert!(l.spans[0].marker);
        assert_eq!(l.spans[1].range, 6..8);
        assert!(l.spans[1].style.bold && l.spans[1].marker);
        assert_eq!(l.spans.last().unwrap().range.end, 15);
        assert!(line("").spans.is_empty());
        assert_eq!(line("plain").spans.len(), 1);
    }

    #[test]
    fn lines_know_where_they_start() {
        let starts: Vec<usize> = lines("a\n\nbc\n").map(|(s, _)| s).collect();
        assert_eq!(starts, vec![0, 2, 3, 6]);
    }
}
