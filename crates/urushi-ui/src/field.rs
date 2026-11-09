//! Painting text fields, and mapping a pointer position back to a place in their text
//! (for click-to-place, drag-to-select and double-click).
//!
//! A single-line field scrolls sideways so the caret stays in view: text that is wider
//! than the box slides left as the caret moves past the right edge. A multiline field
//! wraps its text to the box and scrolls up the same way when the caret goes below it.
//! Each painted field leaves a [`FieldRegion`] in the frame recording where its lines
//! were drawn, so the runner can turn a click into a caret position with [`caret_at`].

use std::ops::Range;

use crate::color::Color;
use crate::geometry::{Point, Rect};
use crate::node::Key;
use crate::scene::{Font, Scene};
use crate::text::TextSystem;

/// Room kept right of the caret when the text scrolls, in logical pixels.
const CARET_MARGIN: f32 = 2.0;

/// Where a text field's text was drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldRegion {
    /// Key of the nearest keyed ancestor (the field widget).
    pub key: Option<Key>,
    /// The text area (inside the padding).
    pub rect: Rect,
    /// Where x = 0 of the text is, after sideways scrolling.
    pub origin_x: f32,
    /// Where the top of the first line is, after scrolling up.
    pub origin_y: f32,
    /// The text shown.
    pub text: String,
    /// The lines shown, as byte ranges of `text` (one for a single-line field).
    pub lines: Vec<Range<usize>>,
    /// Font size.
    pub size: f32,
}

impl FieldRegion {
    /// The line at height `y`, clamped to the first and last.
    fn line_at(&self, y: f32) -> usize {
        let line = TextSystem::line_height(self.size);
        let row = ((y - self.origin_y) / line).floor().max(0.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative, and clamped to the line count"
        )]
        let row = row as usize;
        row.min(self.lines.len().saturating_sub(1))
    }
}

/// The byte offset in `region`'s text nearest to `p`: where a click there puts the
/// caret. Always a character boundary.
pub fn caret_at(text: &mut TextSystem, region: &FieldRegion, p: Point) -> usize {
    let Some(range) = region.lines.get(region.line_at(p.y)).cloned() else {
        return 0;
    };
    let mut line = &region.text[range.clone()];
    // A click past the end of a soft-wrapped line stays on it, before the space it
    // ends with, rather than landing at the start of the next line.
    let wrapped = region.lines.iter().any(|l| l.start == range.end);
    if wrapped && let Some((last, _)) = line.char_indices().next_back() {
        line = &line[..last];
    }
    range.start + offset_at(text, line, region.size, p.x - region.origin_x)
}

/// The character boundary in one line of text nearest to `x` (from its start).
fn offset_at(text: &mut TextSystem, s: &str, size: f32, x: f32) -> usize {
    if x <= 0.0 {
        return 0;
    }
    let mut bounds: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    bounds.push(s.len());
    // Prefix widths only grow, so a binary search finds the first boundary right of x.
    let (mut lo, mut hi) = (0, bounds.len() - 1);
    while lo < hi {
        let mid = usize::midpoint(lo, hi);
        if text.measure(&s[..bounds[mid]], Font::Ui, size) < x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return 0;
    }
    // Snap to whichever side of that character is closer.
    let right = text.measure(&s[..bounds[lo]], Font::Ui, size);
    let left = text.measure(&s[..bounds[lo - 1]], Font::Ui, size);
    if x - left < right - x {
        bounds[lo - 1]
    } else {
        bounds[lo]
    }
}

/// What a field shows.
pub(crate) struct Look<'a> {
    pub(crate) text: &'a str,
    pub(crate) caret: Option<usize>,
    pub(crate) selection: Option<(usize, usize)>,
    pub(crate) placeholder: &'a str,
    pub(crate) colors: [Color; 4],
    pub(crate) size: f32,
    pub(crate) multiline: bool,
}

/// Paints a field into `inner` and says where its text went.
pub(crate) fn paint(
    scene: &mut Scene,
    text: &mut TextSystem,
    inner: Rect,
    look: &Look<'_>,
    key: Option<Key>,
) -> FieldRegion {
    let [color, placeholder_color, caret_color, selection_color] = look.colors;
    let size = look.size;
    let line = TextSystem::line_height(size);
    let lines = if look.multiline {
        text.wrap(look.text, Font::Ui, size, inner.width - CARET_MARGIN)
    } else {
        std::iter::once(0..look.text.len()).collect()
    };
    // The caret is on the last line starting at or before it (at a soft break, the
    // start of the next line).
    let caret = look.caret.map(|c| {
        let row = lines.iter().rposition(|l| l.start <= c).unwrap_or(0);
        (row, width(text, look.text, lines[row].start..c, size))
    });
    let (origin_x, origin_y) = if look.multiline {
        let bottom = caret.map_or(0.0, |(row, _)| row_top(row, line) + line);
        (inner.x, inner.y - (bottom - inner.height).max(0.0))
    } else {
        let scroll = caret.map_or(0.0, |(_, x)| (x - inner.width + CARET_MARGIN).max(0.0));
        (
            inner.x - scroll,
            (inner.y + (inner.height - line) / 2.0).round(),
        )
    };
    scene.push_clip(inner);
    if let Some((start, end)) = look.selection {
        for (row, l) in lines.iter().enumerate() {
            if end <= l.start || start > l.end {
                continue;
            }
            let a = width(text, look.text, l.start..start.max(l.start), size);
            let b = width(text, look.text, l.start..end.min(l.end), size);
            // A selected line break shows as a little room after the line.
            let gap = if end > l.end && look.multiline {
                size * 0.3
            } else {
                0.0
            };
            let y = origin_y + row_top(row, line);
            scene.rect(
                Rect::new(origin_x + a, y + 1.0, b - a + gap, line - 2.0),
                selection_color,
            );
        }
    }
    if look.text.is_empty() {
        scene.text(
            Point::new(origin_x.round(), origin_y),
            look.placeholder.to_owned(),
            Font::Ui,
            size,
            placeholder_color,
        );
    }
    for (row, l) in lines.iter().enumerate() {
        let y = origin_y + row_top(row, line);
        if y + line < inner.y || y > inner.y + inner.height {
            continue;
        }
        scene.text(
            Point::new(origin_x.round(), y.round()),
            look.text[l.clone()].to_owned(),
            Font::Ui,
            size,
            color,
        );
    }
    if let Some((row, x)) = caret {
        let y = origin_y + row_top(row, line);
        scene.rect(
            Rect::new((origin_x + x).round(), y + 2.0, 1.5, line - 4.0),
            caret_color,
        );
    }
    scene.pop_clip();
    FieldRegion {
        key,
        rect: inner,
        origin_x,
        origin_y,
        text: look.text.to_owned(),
        lines,
        size,
    }
}

/// How far below the first line row `row` starts.
fn row_top(row: usize, line: f32) -> f32 {
    #[allow(clippy::cast_precision_loss, reason = "a field has few lines")]
    let row = row as f32;
    row * line
}

/// Width of `range` of `s`.
fn width(text: &mut TextSystem, s: &str, range: Range<usize>, size: f32) -> f32 {
    text.measure(s.get(range).unwrap_or_default(), Font::Ui, size)
}
