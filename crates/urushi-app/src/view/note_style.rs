//! How thread notes look (UR-72, ADR 0058). The notes are Markdown, parsed a line at a
//! time by `urushi_core::markdown`. The editor shows the source with each mark's text
//! styled and the marks dimmed; a card shows a line with the marks left out, in the
//! notes' color.

use urushi_core::LabelColor;
use urushi_core::markdown::{self, BlockKind, Style};
use urushi_ui::{Color, Font, Run, TextStyle, Theme};

/// The color notes are shown in when none was picked: like a sticky note.
pub(super) const DEFAULT_NOTE_COLOR: LabelColor = LabelColor::Yellow;

/// The theme's shade of a thread's notes color (a wire `LabelColor`; 0 or unknown is
/// [`DEFAULT_NOTE_COLOR`]).
pub(super) fn note_color(theme: &Theme, wire: i32) -> Color {
    let color = urushi_proto::convert::color_from_proto(wire).unwrap_or(DEFAULT_NOTE_COLOR);
    theme.labels[color.index()]
}

/// The wire value of the color an editor shows as picked: the default when none is.
pub(super) fn shown_color(wire: i32) -> i32 {
    match urushi_proto::convert::color_from_proto(wire) {
        Some(_) => wire,
        None => urushi_proto::convert::color_to_proto(Some(DEFAULT_NOTE_COLOR)),
    }
}

/// The font for text with `style`, on a heading line or not.
fn font(style: Style, heading: bool) -> Font {
    match (style.code, style.bold || heading, style.italic) {
        (true, ..) => Font::MONO,
        (false, true, true) => Font::UiSemiboldItalic,
        (false, true, false) => Font::UiSemibold,
        (false, false, true) => Font::UiItalic,
        (false, false, false) => Font::Ui,
    }
}

/// The runs that style notes' Markdown source in the editor: marks in the muted color,
/// bold, italic and code in their fonts, links in the accent and underlined, struck
/// text and done tasks struck through, headings bold, and quotes muted.
pub(super) fn editor_runs(theme: &Theme, text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    for (start, line) in markdown::lines(text) {
        let heading = matches!(line.block.kind, BlockKind::Heading(_));
        let done = matches!(line.block.kind, BlockKind::Task { done: true });
        let quote = line.block.kind == BlockKind::Quote;
        for span in line.spans {
            let range = start + span.range.start..start + span.range.end;
            let style = if span.marker {
                TextStyle {
                    font: font(span.style, heading),
                    color: Some(theme.text_muted),
                    ..TextStyle::default()
                }
            } else {
                TextStyle {
                    font: font(span.style, heading),
                    color: if span.style.link {
                        Some(theme.accent)
                    } else if quote || done {
                        Some(theme.text_muted)
                    } else {
                        None
                    },
                    strike: span.style.strike || done,
                    underline: span.style.link,
                }
            };
            if style != TextStyle::default() {
                runs.push(Run::new(range, style));
            }
        }
    }
    runs
}

/// One line of notes as a card shows it: the text with the marks left out, its runs
/// (all in `color`), and its block, which says what to draw before it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Preview {
    /// The text to draw.
    pub(super) text: String,
    /// Its runs.
    pub(super) runs: Vec<Run>,
    /// The line's kind: a task gets a box, a bullet a dot, a numbered item its number.
    pub(super) kind: BlockKind,
}

/// The first line of `notes` with text (not just a marker) as a [`Preview`] in `color`.
pub(super) fn preview(notes: &str, color: Color) -> Option<Preview> {
    notes.lines().find_map(|l| preview_line(l, color))
}

fn preview_line(source: &str, color: Color) -> Option<Preview> {
    let line = markdown::line(source);
    let content = line.spans.iter().filter(|s| !s.marker);
    if content
        .map(|s| &source[s.range.clone()])
        .all(|t| t.trim().is_empty())
    {
        return None;
    }
    let heading = matches!(line.block.kind, BlockKind::Heading(_));
    let done = matches!(line.block.kind, BlockKind::Task { done: true });
    let mut text = match line.block.kind {
        BlockKind::Bullet => "• ".to_owned(),
        BlockKind::Numbered(n) => format!("{n}. "),
        _ => String::new(),
    };
    let mut runs = Vec::new();
    for span in line.spans.iter().filter(|s| !s.marker) {
        let start = text.len();
        text.push_str(&source[span.range.clone()]);
        runs.push(Run::new(
            start..text.len(),
            TextStyle {
                font: font(span.style, heading),
                color: Some(if done { color.with_alpha(0.6) } else { color }),
                strike: span.style.strike || done,
                underline: span.style.link,
            },
        ));
    }
    // Spaces at the end would only push an ellipsis out.
    text.truncate(text.trim_end().len());
    for run in &mut runs {
        run.range.end = run.range.end.min(text.len());
        run.range.start = run.range.start.min(run.range.end);
    }
    runs.retain(|r| !r.range.is_empty());
    if runs.first().is_none_or(|r| r.range.start > 0) {
        let end = runs.first().map_or(text.len(), |r| r.range.start);
        runs.insert(
            0,
            Run::new(
                0..end,
                TextStyle {
                    color: Some(color),
                    ..TextStyle::default()
                },
            ),
        );
    }
    Some(Preview {
        text,
        runs,
        kind: line.block.kind,
    })
}

#[cfg(test)]
mod tests;
