//! Word wrapping: which byte ranges of a text go on which line at a given width. One
//! font ([`TextSystem::wrap`]) or any measure ([`TextSystem::wrap_by`], which styled
//! text uses to measure each run in its own font).

use std::ops::Range;

use super::TextSystem;
use crate::scene::Font;

/// How wide a byte range of the text being wrapped is.
pub type Measure<'a> = dyn FnMut(&mut TextSystem, Range<usize>) -> f32 + 'a;

impl TextSystem {
    /// The lines `text` takes up at `width`: it breaks at each `\n`, and between words
    /// where a line would get wider than `width`. A word too long for a line by itself
    /// is broken between characters. Each line is a byte range of `text`, without the
    /// `\n`. Spaces at a soft break stay at the end of the line before, so the ranges of
    /// one paragraph touch.
    pub fn wrap(&mut self, text: &str, font: Font, size: f32, width: f32) -> Vec<Range<usize>> {
        self.wrap_first(text, font, size, width, usize::MAX)
    }

    /// The first `max_lines` lines of [`Self::wrap`], without measuring the rest of the
    /// text. Boxes that show a few lines of a long text (a card's notes) wrap only what
    /// they show, so a long description at a narrow width stays cheap.
    pub fn wrap_first(
        &mut self,
        text: &str,
        font: Font,
        size: f32,
        width: f32,
        max_lines: usize,
    ) -> Vec<Range<usize>> {
        self.wrap_by(text, width, max_lines, &mut |ts, range| {
            ts.measure(&text[range], font, size)
        })
    }

    /// [`Self::wrap_first`] with widths from `measure`, given byte ranges of `text`.
    pub fn wrap_by(
        &mut self,
        text: &str,
        width: f32,
        max_lines: usize,
        measure: &mut Measure<'_>,
    ) -> Vec<Range<usize>> {
        let width = width.max(1.0);
        let mut lines = Vec::new();
        let mut base = 0;
        for paragraph in text.split('\n') {
            if lines.len() >= max_lines {
                break;
            }
            let p = base..base + paragraph.len();
            self.wrap_paragraph(text, p, (width, max_lines), measure, &mut lines);
            base += paragraph.len() + 1;
        }
        lines.truncate(max_lines);
        lines
    }

    fn wrap_paragraph(
        &mut self,
        text: &str,
        p: Range<usize>,
        (width, max_lines): (f32, usize),
        measure: &mut Measure<'_>,
        lines: &mut Vec<Range<usize>>,
    ) {
        // Each token is a word and the spaces after it, as byte ranges of `text`.
        let mut tokens = Vec::new();
        let (mut token_start, mut after_space) = (p.start, false);
        for (i, c) in text[p.clone()].char_indices() {
            let space = c.is_whitespace();
            if after_space && !space {
                tokens.push(token_start..p.start + i);
                token_start = p.start + i;
            }
            after_space = space;
        }
        if !p.is_empty() {
            tokens.push(token_start..p.end);
        }
        let mut start = p.start;
        let mut i = 0;
        while i < tokens.len() {
            if lines.len() >= max_lines {
                return;
            }
            let token = tokens[i].clone();
            let end = start + text[start..token.end].trim_end().len();
            if measure(self, start..end) <= width {
                i += 1;
            } else if token.start > start {
                // The word goes on the next line.
                lines.push(start..token.start);
                start = token.start;
            } else {
                // A word wider than the line: as many characters as fit (at least one).
                let cut = self.longest_fit(text, start..token.end, width, measure);
                if cut == token.end {
                    // One character wider than the line by itself: it ends this line.
                    i += 1;
                } else {
                    lines.push(start..cut);
                    start = cut;
                    tokens[i].start = start;
                }
            }
        }
        lines.push(start..p.end);
    }

    /// Where the longest prefix of `range` (at least one character) that fits in
    /// `width` ends.
    fn longest_fit(
        &mut self,
        text: &str,
        range: Range<usize>,
        width: f32,
        measure: &mut Measure<'_>,
    ) -> usize {
        let bounds: Vec<usize> = text[range.clone()]
            .char_indices()
            .map(|(i, _)| range.start + i)
            .skip(1)
            .collect();
        let first = bounds.first().copied().unwrap_or(range.end);
        // Prefix widths only grow, so binary search the last boundary that fits.
        let (mut lo, mut hi) = (0, bounds.len());
        while lo < hi {
            let mid = usize::midpoint(lo, hi);
            if measure(self, range.start..bounds[mid]) <= width {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 { first } else { bounds[lo - 1] }
    }
}
