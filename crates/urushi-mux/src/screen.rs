//! Renderer-neutral screen model: what a client needs to draw one pane.
//!
//! These types don't depend on any emulator or wire format. The emulator adapter
//! produces them, and the daemon turns them into protocol messages.

/// Terminal size in character cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TermSize {
    /// Columns (≥ 1).
    pub cols: u16,
    /// Rows (≥ 1).
    pub rows: u16,
}

impl TermSize {
    /// The traditional 80×24 default.
    pub const DEFAULT: Self = Self { cols: 80, rows: 24 };

    /// Builds a size, raising zeros to 1 (a zero-sized terminal is meaningless).
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }
}

/// A cell colour. `Default` means "the theme's default foreground or background".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    /// The theme's default for this slot.
    #[default]
    Default,
    /// Palette index 0–255. Indices 0–15 are the theme's ANSI colours.
    Indexed(u8),
    /// Direct colour.
    Rgb(u8, u8, u8),
}

/// Style flag bits. The values match `StyleFlag` in `terminal.proto`.
pub mod flags {
    /// Bold weight.
    pub const BOLD: u32 = 1;
    /// Italic.
    pub const ITALIC: u32 = 2;
    /// Any underline.
    pub const UNDERLINE: u32 = 4;
    /// Swap foreground and background.
    pub const INVERSE: u32 = 8;
    /// Faint.
    pub const DIM: u32 = 16;
    /// Strikethrough.
    pub const STRIKETHROUGH: u32 = 32;
    /// Every character in the span is double width.
    pub const WIDE: u32 = 64;
    /// Concealed text.
    pub const HIDDEN: u32 = 128;
}

/// Visual attributes shared by a run of cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    /// Foreground.
    pub fg: Color,
    /// Background.
    pub bg: Color,
    /// Bitwise OR of [`flags`] values.
    pub flags: u32,
}

impl Style {
    /// Whether this is the plain default style.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A run of characters with one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    /// The characters (with any combining marks).
    pub text: String,
    /// Their style.
    pub style: Style,
}

/// One screen line, as spans starting at column 0. Columns past the last span are
/// blank and default-styled (trailing blanks are trimmed to save bandwidth).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// Spans in column order.
    pub spans: Vec<Span>,
}

impl Line {
    /// The line's characters, without styling.
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Cursor shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    /// Filled block.
    #[default]
    Block,
    /// Underscore.
    Underline,
    /// Vertical bar.
    Bar,
}

/// Cursor state, in viewport coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// Viewport line.
    pub line: u16,
    /// Column.
    pub col: u16,
    /// Hidden by the application, or scrolled out of view.
    pub visible: bool,
    /// Shape requested by the application.
    pub shape: CursorShape,
}

/// Terminal modes that affect how a client sends input.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent DEC private modes"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// DECCKM: arrow keys send `ESC O x`.
    pub app_cursor: bool,
    /// Pastes are wrapped in `ESC [200~ … ESC [201~`.
    pub bracketed_paste: bool,
    /// Which mouse events the application asked for (DEC modes 1000/1002/1003).
    pub mouse: MouseMode,
    /// How mouse reports are encoded (DEC modes 1006 and 1005).
    pub mouse_encoding: MouseEncoding,
    /// The alternate screen (full-screen apps) is active.
    pub alt_screen: bool,
    /// DEC 1007: on the alternate screen, the wheel sends arrow keys.
    pub alternate_scroll: bool,
}

impl Modes {
    /// Whether the application wants any mouse events.
    pub fn mouse_reporting(&self) -> bool {
        self.mouse != MouseMode::Off
    }
}

/// Which mouse events the application receives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    /// None: the terminal handles the mouse itself (selection, scrollback).
    #[default]
    Off,
    /// Presses, releases and the wheel (1000).
    Click,
    /// Also movement while a button is held (1002).
    Drag,
    /// Also all movement (1003).
    Motion,
}

/// How mouse reports are encoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseEncoding {
    /// `ESC [ M` plus three bytes; positions above 223 can't be reported.
    #[default]
    X10,
    /// `ESC [ M` plus three UTF-8 characters (1005).
    Utf8,
    /// `ESC [ < b ; x ; y M/m` (1006), what modern applications use.
    Sgr,
}

/// Changed screen content since some earlier version. See [`crate::SessionHandle::frame_since`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenFrame {
    /// The version this frame brings the client up to.
    pub version: u64,
    /// `true` if `lines` holds every line and the client should discard what it has.
    pub full: bool,
    /// Current size.
    pub size: TermSize,
    /// `(viewport line, content)` for each changed line.
    pub lines: Vec<(u16, Line)>,
    /// Cursor.
    pub cursor: Cursor,
    /// Input-relevant modes.
    pub modes: Modes,
    /// Window title set by the application.
    pub title: String,
    /// Lines scrolled back from the live view.
    pub display_offset: u32,
    /// Lines of scrollback available.
    pub history_size: u32,
}
