//! Text in several styles: runs of a string, each with its own font, color, strikethrough
//! or underline. Fields use it to draw formatted source (thread notes, ADR 0058), and
//! [`rich_line`] draws one formatted line, cut off with an ellipsis.
//!
//! The scene has one font and one color per text primitive, so a styled line becomes one
//! primitive per run, placed side by side by measuring each run. Measuring, wrapping and
//! placing a caret all go through [`Styled::width`], so they always agree with what is
//! drawn. All runs share one size, which keeps every line the same height.

use std::ops::Range;

use crate::color::Color;
use crate::geometry::{Point, Rect};
use crate::node::{Length, Node};
use crate::scene::{Font, Scene};
use crate::text::TextSystem;

/// How a run of text looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Its font.
    pub font: Font,
    /// Its color; `None` uses the text's own color.
    pub color: Option<Color>,
    /// Struck through.
    pub strike: bool,
    /// Underlined.
    pub underline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: Font::Ui,
            color: None,
            strike: false,
            underline: false,
        }
    }
}

/// A byte range of a text and its style.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The bytes it covers (on character boundaries).
    pub range: Range<usize>,
    /// How they look.
    pub style: TextStyle,
}

impl Run {
    /// A run of `range` in `style`.
    pub fn new(range: Range<usize>, style: TextStyle) -> Self {
        Self { range, style }
    }
}

/// A text with its runs. The runs are in order and don't overlap; bytes no run covers
/// use [`TextStyle::default`]. With no runs at all, the text is measured in one piece,
/// exactly as plain text is.
#[derive(Clone, Copy, Debug)]
pub struct Styled<'a> {
    /// The text.
    pub text: &'a str,
    /// Its runs.
    pub runs: &'a [Run],
}

impl<'a> Styled<'a> {
    /// `text` with `runs`.
    pub fn new(text: &'a str, runs: &'a [Run]) -> Self {
        Self { text, runs }
    }

    /// The pieces of `range` in each style, in order.
    pub fn segments(&self, range: Range<usize>) -> Vec<(Range<usize>, TextStyle)> {
        let mut out = Vec::new();
        let mut at = range.start;
        for run in self.runs {
            if run.range.end <= at {
                continue;
            }
            if at >= range.end || run.range.start >= range.end {
                break;
            }
            if run.range.start > at {
                out.push((at..run.range.start, TextStyle::default()));
            }
            let end = run.range.end.min(range.end);
            out.push((at.max(run.range.start)..end, run.style));
            at = end;
        }
        if at < range.end {
            out.push((at..range.end, TextStyle::default()));
        }
        out
    }

    /// The width of `range` at `size`.
    pub fn width(&self, ts: &mut TextSystem, range: Range<usize>, size: f32) -> f32 {
        let Some(text) = self.text.get(range.clone()) else {
            return 0.0;
        };
        if self.runs.is_empty() {
            return ts.measure(text, Font::Ui, size);
        }
        self.segments(range)
            .into_iter()
            .map(|(r, style)| ts.measure(&self.text[r], style.font, size))
            .sum()
    }

    /// The text's lines at `width` (see [`TextSystem::wrap`]), measured in each run's
    /// font.
    pub fn wrap(&self, ts: &mut TextSystem, size: f32, width: f32) -> Vec<Range<usize>> {
        ts.wrap_by(self.text, width, usize::MAX, &mut |ts, r| {
            self.width(ts, r, size)
        })
    }

    /// Draws `range` (one line) with its top-left at `at`, in `color` where a run has
    /// none of its own.
    pub fn paint(
        &self,
        scene: &mut Scene,
        ts: &mut TextSystem,
        at: Point,
        (range, size, color): (Range<usize>, f32, Color),
    ) {
        if self.runs.is_empty() {
            let text = self.text.get(range).unwrap_or_default().to_owned();
            scene.text(at, text, Font::Ui, size, color);
            return;
        }
        let mut x = at.x;
        for (r, style) in self.segments(range) {
            let text = &self.text[r];
            let w = ts.measure(text, style.font, size);
            let c = style.color.unwrap_or(color);
            decorate(
                scene,
                Rect::new(x, at.y, w, TextSystem::line_height(size)),
                style,
                c,
            );
            scene.text(
                Point::new(x.round(), at.y),
                text.to_owned(),
                style.font,
                size,
                c,
            );
            x += w;
        }
    }
}

/// Strikethrough and underline lines for a run drawn in `rect`.
fn decorate(scene: &mut Scene, rect: Rect, style: TextStyle, color: Color) {
    let thick = 1.0;
    if style.strike {
        let y = (rect.y + rect.height * 0.52).round();
        scene.rect(Rect::new(rect.x, y, rect.width, thick), color);
    }
    if style.underline {
        let y = (rect.y + rect.height - 3.0).round();
        scene.rect(Rect::new(rect.x, y, rect.width, thick), color);
    }
}

/// One line of styled text, filling its row's width, at `size` in `color` (where a run
/// has none of its own). When it doesn't fit, the run where it runs out is cut short
/// with an ellipsis and the rest is left out.
pub fn rich_line<M>(text: &str, runs: Vec<Run>, size: f32, color: Color) -> Node<M> {
    let text = text.to_owned();
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(TextSystem::line_height(size)))
        .paint(move |rect, scene, ts| {
            let styled = Styled::new(&text, &runs);
            scene.push_clip(rect);
            let mut x = rect.x;
            let right = rect.x + rect.width;
            for (r, style) in styled.segments(0..text.len()) {
                let c = style.color.unwrap_or(color);
                let piece = &text[r];
                let w = ts.measure(piece, style.font, size);
                let (shown, w, last) = if x + w <= right {
                    (piece.to_owned(), w, false)
                } else {
                    let cut = ts.truncate(piece, style.font, size, (right - x).max(0.0));
                    let cw = ts.measure(&cut, style.font, size);
                    (cut, cw, true)
                };
                let at = Rect::new(x, rect.y, w, rect.height);
                decorate(scene, at, style, c);
                scene.text(Point::new(x.round(), rect.y), shown, style.font, size, c);
                x += w;
                if last {
                    break;
                }
            }
            scene.pop_clip();
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(range: Range<usize>) -> Run {
        Run::new(
            range,
            TextStyle {
                font: Font::UiSemibold,
                ..TextStyle::default()
            },
        )
    }

    #[test]
    fn segments_fill_the_gaps_between_runs() {
        let runs = [bold(2..4), bold(6..8)];
        let s = Styled::new("abcdefghij", &runs);
        let pieces: Vec<(Range<usize>, Font)> = s
            .segments(1..7)
            .into_iter()
            .map(|(r, st)| (r, st.font))
            .collect();
        assert_eq!(
            pieces,
            vec![
                (1..2, Font::Ui),
                (2..4, Font::UiSemibold),
                (4..6, Font::Ui),
                (6..7, Font::UiSemibold),
            ]
        );
        assert!(s.segments(3..3).is_empty());
    }

    #[test]
    fn widths_add_up_by_run_and_plain_text_is_measured_whole() {
        let mut ts = TextSystem::bundled_only();
        let plain = Styled::new("Hello world", &[]);
        assert!(
            (plain.width(&mut ts, 0..11, 13.0) - ts.measure("Hello world", Font::Ui, 13.0)).abs()
                < 0.01
        );
        let runs = [bold(0..5)];
        let styled = Styled::new("Hello world", &runs);
        let whole = styled.width(&mut ts, 0..11, 13.0);
        let parts =
            ts.measure("Hello", Font::UiSemibold, 13.0) + ts.measure(" world", Font::Ui, 13.0);
        assert!((whole - parts).abs() < 0.01);
        assert!(whole > plain.width(&mut ts, 0..11, 13.0));
    }

    #[test]
    fn painting_draws_one_text_per_run_with_its_lines() {
        let mut ts = TextSystem::bundled_only();
        let runs = [Run::new(
            0..3,
            TextStyle {
                font: Font::UiItalic,
                color: Some(Color::hex(0xff0000)),
                strike: true,
                underline: false,
            },
        )];
        let mut scene = Scene::new();
        Styled::new("abc def", &runs).paint(
            &mut scene,
            &mut ts,
            Point::new(0.0, 0.0),
            (0..7, 13.0, Color::hex(0xffffff)),
        );
        let dump = scene.dump();
        assert!(dump.contains("\"abc\" ui-italic 13 #ff0000"), "{dump}");
        assert!(dump.contains("\" def\" ui 13 #ffffff"), "{dump}");
        assert_eq!(dump.matches("quad").count(), 1, "{dump}");
    }

    #[test]
    fn styled_text_wraps_with_its_own_widths() {
        let mut ts = TextSystem::bundled_only();
        let runs = [bold(0..9)];
        let s = Styled::new("aaaa bbbb cccc", &runs);
        let one = ts.measure("aaaa bbbb", Font::UiSemibold, 13.0);
        let lines = s.wrap(&mut ts, 13.0, one + 1.0);
        assert_eq!(lines, vec![0..10, 10..14]);
    }
}
