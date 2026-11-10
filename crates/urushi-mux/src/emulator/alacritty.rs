//! [`Emulator`] backed by `alacritty_terminal` (Apache-2.0).

use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll as GridScroll};
use alacritty_terminal::index::{Column, Line as GridLine};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as VtColor, CursorShape as VtCursorShape, NamedColor, Processor, Rgb,
};

use super::{Damage, Emulator, EmulatorEvent, Scroll};
use crate::screen::{
    Color, Cursor, CursorShape, Line, Modes, MouseEncoding, MouseMode, Span, Style, TermSize, flags,
};

/// Scrollback kept per pane.
const SCROLLBACK_LINES: usize = 10_000;

/// Forwards alacritty's events through a channel, so `Term` stays `Send` and lock-free.
struct Listener(mpsc::Sender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // The receiver lives as long as the emulator, so a failed send can only happen
        // during teardown, when the event no longer matters.
        let _ = self.0.send(event);
    }
}

struct Size(TermSize);

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        usize::from(self.0.rows)
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.0.cols)
    }
}

/// An emulator using alacritty's terminal state machine.
pub struct AlacrittyEmulator {
    term: Term<Listener>,
    parser: Processor,
    /// Notifications alacritty doesn't report (OSC 9 and 777).
    notices: super::notices::Notices,
    events: mpsc::Receiver<Event>,
    size: TermSize,
}

impl AlacrittyEmulator {
    /// A blank emulator of the given size.
    pub fn new(size: TermSize) -> Self {
        let (tx, rx) = mpsc::channel();
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &Size(size), Listener(tx)),
            parser: Processor::new(),
            notices: super::notices::Notices::default(),
            events: rx,
            size,
        }
    }

    fn translate(&self, event: Event) -> Option<EmulatorEvent> {
        match event {
            Event::PtyWrite(text) => Some(EmulatorEvent::Reply(text.into_bytes())),
            Event::Title(title) => Some(EmulatorEvent::Title(Some(title))),
            Event::ResetTitle => Some(EmulatorEvent::Title(None)),
            Event::Bell => Some(EmulatorEvent::Bell),
            Event::TextAreaSizeRequest(format) => Some(EmulatorEvent::Reply(
                format(WindowSize {
                    num_lines: self.size.rows,
                    num_cols: self.size.cols,
                    cell_width: 0,
                    cell_height: 0,
                })
                .into_bytes(),
            )),
            Event::ColorRequest(index, format) => Some(EmulatorEvent::Reply(
                format(default_rgb(index)).into_bytes(),
            )),
            _ => None,
        }
    }
}

impl Emulator for AlacrittyEmulator {
    fn advance(&mut self, bytes: &[u8]) -> Vec<EmulatorEvent> {
        self.parser.advance(&mut self.term, bytes);
        let pending: Vec<Event> = self.events.try_iter().collect();
        let mut events: Vec<EmulatorEvent> = pending
            .into_iter()
            .filter_map(|e| self.translate(e))
            .collect();
        events.extend(self.notices.advance(bytes).into_iter().map(|n| match n {
            super::notices::Notice::Attention(kind) => EmulatorEvent::Attention(kind),
            super::notices::Notice::Ready => EmulatorEvent::Ready,
        }));
        events
    }

    fn resize(&mut self, size: TermSize) {
        if size != self.size {
            self.size = size;
            self.term.resize(Size(size));
        }
    }

    fn size(&self) -> TermSize {
        self.size
    }

    fn take_damage(&mut self) -> Damage {
        let damage = match self.term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(lines) => Damage::Lines(
                lines
                    .filter_map(|l| u16::try_from(l.line).ok())
                    .filter(|l| *l < self.size.rows)
                    .collect(),
            ),
        };
        self.term.reset_damage();
        damage
    }

    fn line(&self, viewport_line: u16) -> Line {
        let grid = self.term.grid();
        let offset = i32::try_from(grid.display_offset()).unwrap_or(0);
        let row = &grid[GridLine(i32::from(viewport_line) - offset)];
        let cells = (0..grid.columns()).map(|c| &row[Column(c)]);
        line_from_cells(cells)
    }

    fn cursor(&self) -> Cursor {
        let grid = self.term.grid();
        let point = grid.cursor.point;
        let offset = grid.display_offset();
        let line = usize::try_from(point.line.0).unwrap_or(0) + offset;
        let style = self.term.cursor_style();
        let shown = self.term.mode().contains(TermMode::SHOW_CURSOR)
            && style.shape != VtCursorShape::Hidden;
        Cursor {
            line: u16::try_from(line).unwrap_or(u16::MAX),
            col: u16::try_from(point.column.0).unwrap_or(u16::MAX),
            visible: shown && line < usize::from(self.size.rows),
            shape: match style.shape {
                VtCursorShape::Underline => CursorShape::Underline,
                VtCursorShape::Beam => CursorShape::Bar,
                _ => CursorShape::Block,
            },
        }
    }

    fn modes(&self) -> Modes {
        let mode = self.term.mode();
        Modes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            mouse: if mode.contains(TermMode::MOUSE_MOTION) {
                MouseMode::Motion
            } else if mode.contains(TermMode::MOUSE_DRAG) {
                MouseMode::Drag
            } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
                MouseMode::Click
            } else {
                MouseMode::Off
            },
            mouse_encoding: if mode.contains(TermMode::SGR_MOUSE) {
                MouseEncoding::Sgr
            } else if mode.contains(TermMode::UTF8_MOUSE) {
                MouseEncoding::Utf8
            } else {
                MouseEncoding::X10
            },
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
        }
    }

    fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(match scroll {
            Scroll::Lines(n) => GridScroll::Delta(n),
            Scroll::Bottom => GridScroll::Bottom,
        });
    }

    fn display_offset(&self) -> u32 {
        u32::try_from(self.term.grid().display_offset()).unwrap_or(u32::MAX)
    }

    fn history_size(&self) -> u32 {
        u32::try_from(self.term.grid().history_size()).unwrap_or(u32::MAX)
    }

    fn history(&self, max_lines: usize) -> Vec<Line> {
        let grid = self.term.grid();
        let line = |l: i32| {
            let row = &grid[GridLine(l)];
            line_from_cells((0..grid.columns()).map(|c| &row[Column(c)]))
        };
        let oldest = -i32::try_from(grid.history_size()).unwrap_or(0);
        // One past the last line with something on it.
        let mut end = i32::from(self.size.rows);
        while end > oldest && line(end - 1).spans.is_empty() {
            end -= 1;
        }
        let first = oldest.max(end - i32::try_from(max_lines).unwrap_or(i32::MAX));
        (first..end).map(line).collect()
    }
}

/// Groups cells into styled spans, skipping wide-character spacers and trimming
/// trailing default blanks.
fn line_from_cells<'a>(cells: impl Iterator<Item = &'a Cell>) -> Line {
    let mut spans: Vec<Span> = Vec::new();
    let mut trailing_blank_len = 0usize; // bytes of trailing default blanks in the last span
    for cell in cells {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let style = style_of(cell);
        let blank = cell.c == ' ' && style.is_default() && cell.zerowidth().is_none();
        match spans.last_mut() {
            Some(last) if last.style == style => {}
            _ => {
                trailing_blank_len = 0;
                spans.push(Span {
                    text: String::new(),
                    style,
                });
            }
        }
        if let Some(last) = spans.last_mut() {
            last.text.push(cell.c);
            if let Some(marks) = cell.zerowidth() {
                last.text.extend(marks);
            }
            trailing_blank_len = if blank { trailing_blank_len + 1 } else { 0 };
        }
    }
    if let Some(last) = spans.last_mut()
        && last.style.is_default()
    {
        last.text.truncate(last.text.len() - trailing_blank_len);
        if last.text.is_empty() {
            spans.pop();
        }
    }
    Line { spans }
}

fn style_of(cell: &Cell) -> Style {
    let f = cell.flags;
    let mut bits = 0;
    for (flag, bit) in [
        (Flags::BOLD, flags::BOLD),
        (Flags::ITALIC, flags::ITALIC),
        (Flags::ALL_UNDERLINES, flags::UNDERLINE),
        (Flags::INVERSE, flags::INVERSE),
        (Flags::DIM, flags::DIM),
        (Flags::STRIKEOUT, flags::STRIKETHROUGH),
        (Flags::WIDE_CHAR, flags::WIDE),
        (Flags::HIDDEN, flags::HIDDEN),
    ] {
        if f.intersects(flag) {
            bits |= bit;
        }
    }
    let (fg, dim_from_color) = color_of(cell.fg);
    if dim_from_color {
        bits |= flags::DIM;
    }
    Style {
        fg,
        bg: color_of(cell.bg).0,
        flags: bits,
    }
}

/// Maps an alacritty colour to ours. The flag is `true` for the "dim" named colours,
/// which we express as the base colour plus the DIM flag.
fn color_of(color: VtColor) -> (Color, bool) {
    match color {
        VtColor::Spec(rgb) => (Color::Rgb(rgb.r, rgb.g, rgb.b), false),
        VtColor::Indexed(i) => (Color::Indexed(i), false),
        VtColor::Named(named) => {
            let n = named as usize;
            if n < 16 {
                (Color::Indexed(u8::try_from(n).unwrap_or(7)), false)
            } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&n) {
                (
                    Color::Indexed(u8::try_from(n - NamedColor::DimBlack as usize).unwrap_or(7)),
                    true,
                )
            } else {
                (Color::Default, named == NamedColor::DimForeground)
            }
        }
    }
}

/// Colours reported to applications that query them (OSC 4/10/11). These are neutral
/// dark-theme defaults. The client's theme decides what is actually drawn.
fn default_rgb(index: usize) -> Rgb {
    const ANSI: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x21),
        (0xcc, 0x66, 0x66),
        (0xb5, 0xbd, 0x68),
        (0xf0, 0xc6, 0x74),
        (0x81, 0xa2, 0xbe),
        (0xb2, 0x94, 0xbb),
        (0x8a, 0xbe, 0xb7),
        (0xc5, 0xc8, 0xc6),
        (0x66, 0x66, 0x66),
        (0xd5, 0x4e, 0x53),
        (0xb9, 0xca, 0x4a),
        (0xe7, 0xc5, 0x47),
        (0x7a, 0xa6, 0xda),
        (0xc3, 0x97, 0xd8),
        (0x70, 0xc0, 0xb1),
        (0xea, 0xea, 0xea),
    ];
    let (r, g, b) = match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let i = index - 16;
            let level = |v: usize| {
                if v == 0 {
                    0
                } else {
                    u8::try_from(55 + v * 40).unwrap_or(255)
                }
            };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let v = u8::try_from(8 + (index - 232) * 10).unwrap_or(255);
            (v, v, v)
        }
        257 => (0x1a, 0x1b, 0x1e), // background
        _ => (0xd8, 0xd8, 0xd8),   // foreground, cursor
    };
    Rgb { r, g, b }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emu(cols: u16, rows: u16) -> AlacrittyEmulator {
        AlacrittyEmulator::new(TermSize::new(cols, rows))
    }

    #[test]
    fn plain_text_and_trailing_blank_trimming() {
        let mut e = emu(20, 3);
        e.advance(b"hello\r\nworld");
        assert_eq!(e.line(0).text(), "hello");
        assert_eq!(e.line(1).text(), "world");
        assert!(e.line(2).spans.is_empty());
        let c = e.cursor();
        assert_eq!((c.line, c.col, c.visible), (1, 5, true));
    }

    #[test]
    fn sgr_styles_become_spans() {
        let mut e = emu(20, 2);
        e.advance(b"\x1b[1;31mred\x1b[0m plain \x1b[38;2;1;2;3mrgb");
        let line = e.line(0);
        assert_eq!(line.spans[0].text, "red");
        assert_eq!(line.spans[0].style.fg, Color::Indexed(1));
        assert_ne!(line.spans[0].style.flags & flags::BOLD, 0);
        assert_eq!(line.spans[1].text, " plain ");
        assert_eq!(line.spans[2].style.fg, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn wide_characters_skip_spacers() {
        let mut e = emu(10, 1);
        e.advance("a世b".as_bytes());
        let line = e.line(0);
        assert_eq!(line.text(), "a世b");
        let wide = line.spans.iter().find(|s| s.text == "世").unwrap();
        assert_ne!(wide.style.flags & flags::WIDE, 0);
    }

    #[test]
    fn damage_tracks_changed_lines() {
        let mut e = emu(10, 4);
        assert_eq!(
            e.take_damage(),
            Damage::Full,
            "new terminals start fully damaged"
        );
        e.advance(b"\x1b[3;1Hx");
        match e.take_damage() {
            Damage::Lines(lines) => assert!(lines.contains(&2)),
            Damage::Full => panic!("expected partial damage"),
        }
        e.resize(TermSize::new(12, 4));
        assert_eq!(e.take_damage(), Damage::Full);
    }

    #[test]
    fn replies_to_device_status_report() {
        let mut e = emu(10, 3);
        let events = e.advance(b"\x1b[6n");
        assert!(events.contains(&EmulatorEvent::Reply(b"\x1b[1;1R".to_vec())));
    }

    #[test]
    fn title_and_bell_events() {
        let mut e = emu(10, 3);
        let events = e.advance(b"\x1b]2;my title\x07\x07");
        assert!(events.contains(&EmulatorEvent::Title(Some("my title".into()))));
        assert!(events.contains(&EmulatorEvent::Bell));
    }

    #[test]
    fn modes_and_scrollback() {
        let mut e = emu(10, 2);
        e.advance(b"\x1b[?1h\x1b[?2004h");
        let m = e.modes();
        assert!(m.app_cursor && m.bracketed_paste && !m.alt_screen);
        e.advance(b"1\r\n2\r\n3\r\n4");
        assert_eq!(e.history_size(), 2);
        e.scroll(Scroll::Lines(1));
        assert_eq!(e.display_offset(), 1);
        assert_eq!(e.line(0).text(), "2");
        assert!(!e.cursor().visible || e.cursor().line == 2);
        e.scroll(Scroll::Bottom);
        assert_eq!(e.line(1).text(), "4");
    }

    #[test]
    fn default_palette_matches_xterm_cube() {
        let rgb = default_rgb(16 + 36 * 5 + 6 * 5 + 5);
        assert_eq!((rgb.r, rgb.g, rgb.b), (255, 255, 255));
        let gray = default_rgb(232);
        assert_eq!((gray.r, gray.g, gray.b), (8, 8, 8));
    }
}
