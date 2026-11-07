//! A client-side copy of one pane's screen, built from `ScreenUpdate`s.

use urushi_proto::v1;

/// Style flag: every character of the span is two columns wide.
const WIDE: u32 = v1::StyleFlag::Wide as u32;

/// One drawable cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The character, with any combining marks. Empty for the right half of a wide character.
    pub text: String,
    /// Columns occupied (1, or 2 for wide characters).
    pub width: u8,
    /// Style (`None` = defaults).
    pub style: Option<v1::Style>,
}

/// The current screen of a pane, as the daemon last reported it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenState {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Each line's spans (see `docs/protocol.md` for the encoding).
    pub lines: Vec<Vec<v1::Span>>,
    /// Cursor.
    pub cursor: v1::Cursor,
    /// Input-relevant terminal modes.
    pub modes: v1::TerminalModes,
    /// Title set by the application.
    pub title: String,
    /// Lines scrolled back from the live view.
    pub display_offset: u32,
    /// Scrollback available.
    pub history_size: u32,
    /// Sequence number of the last applied update.
    pub seq: u64,
}

impl ScreenState {
    /// Applies an update: a full replacement, or the changed lines.
    pub fn apply(&mut self, update: v1::ScreenUpdate) {
        self.cols = u16::try_from(update.cols).unwrap_or(u16::MAX);
        self.rows = u16::try_from(update.rows).unwrap_or(u16::MAX);
        let rows = usize::from(self.rows);
        if update.full {
            self.lines = vec![Vec::new(); rows];
        } else {
            self.lines.resize(rows, Vec::new());
        }
        for line in update.lines {
            if let Some(slot) = usize::try_from(line.index)
                .ok()
                .and_then(|i| self.lines.get_mut(i))
            {
                *slot = line.spans;
            }
        }
        if let Some(cursor) = update.cursor {
            self.cursor = cursor;
        }
        if let Some(modes) = update.modes {
            self.modes = modes;
        }
        self.title = update.title;
        self.display_offset = update.display_offset;
        self.history_size = update.history_size;
        self.seq = update.seq;
    }

    /// Text of line `index`, without trailing blanks.
    pub fn line_text(&self, index: usize) -> String {
        self.lines
            .get(index)
            .map(|spans| spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .unwrap_or_default()
    }

    /// Text of every line.
    pub fn text(&self) -> Vec<String> {
        (0..self.lines.len()).map(|i| self.line_text(i)).collect()
    }

    /// The cells of line `index`, left to right, up to `cols` columns. Wide characters
    /// are followed by an empty spacer cell, so cell index equals column.
    pub fn cells(&self, index: usize) -> Vec<Cell> {
        let mut cells = Vec::with_capacity(usize::from(self.cols));
        let Some(spans) = self.lines.get(index) else {
            return cells;
        };
        for span in spans {
            let wide = span.style.as_ref().is_some_and(|s| s.flags & WIDE != 0);
            let mut chars = span.text.chars().peekable();
            while let Some(c) = chars.next() {
                let mut text = c.to_string();
                // Attach following combining marks to this cell.
                while let Some(next) = chars.peek() {
                    if is_combining(*next) {
                        text.push(*next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                cells.push(Cell {
                    text,
                    width: if wide { 2 } else { 1 },
                    style: span.style,
                });
                if wide {
                    cells.push(Cell {
                        text: String::new(),
                        width: 0,
                        style: span.style,
                    });
                }
            }
        }
        cells.truncate(usize::from(self.cols));
        cells
    }
}

/// Combining marks the emulator stores with the previous cell.
fn is_combining(c: char) -> bool {
    matches!(u32::from(c), 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F | 0x200D | 0xFE0F)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, flags: u32) -> v1::Span {
        v1::Span {
            text: text.into(),
            style: Some(v1::Style {
                fg: None,
                bg: None,
                flags,
            }),
        }
    }

    #[test]
    fn full_then_partial_updates() {
        let mut s = ScreenState::default();
        s.apply(v1::ScreenUpdate {
            full: true,
            cols: 10,
            rows: 3,
            seq: 1,
            lines: vec![v1::ScreenLine {
                index: 0,
                spans: vec![span("hello", 0)],
            }],
            ..Default::default()
        });
        assert_eq!(s.text(), vec!["hello", "", ""]);
        s.apply(v1::ScreenUpdate {
            full: false,
            cols: 10,
            rows: 3,
            seq: 2,
            lines: vec![v1::ScreenLine {
                index: 2,
                spans: vec![span("bye", 0)],
            }],
            ..Default::default()
        });
        assert_eq!(s.text(), vec!["hello", "", "bye"]);
        assert_eq!(s.seq, 2);
    }

    #[test]
    fn cells_expand_wide_characters_and_keep_marks() {
        let mut s = ScreenState {
            cols: 10,
            rows: 1,
            ..Default::default()
        };
        s.lines = vec![vec![span("a", 0), span("世", WIDE), span("e\u{301}", 0)]];
        let cells = s.cells(0);
        let texts: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "世", "", "e\u{301}"]);
        assert_eq!(cells[1].width, 2);
    }
}
