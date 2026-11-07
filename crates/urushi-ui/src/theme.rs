//! Design tokens. Widgets and screens read every colour and size from a [`Theme`], so
//! restyling (or adding a light theme) never touches layout code.

use crate::color::Color;

/// Colours, sizes and the terminal palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Window background.
    pub background: Color,
    /// Sidebar background.
    pub sidebar: Color,
    /// Raised surfaces: tab bar, dialogs, inputs.
    pub surface: Color,
    /// Hovered interactive element.
    pub hover: Color,
    /// Selected item (current thread, active tab).
    pub selected: Color,
    /// Hairlines and dividers.
    pub border: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text.
    pub text_muted: Color,
    /// Accent (focus rings, primary buttons).
    pub accent: Color,
    /// Text on accent backgrounds.
    pub on_accent: Color,
    /// Destructive actions.
    pub danger: Color,
    /// Success / connected.
    pub success: Color,
    /// Warnings.
    pub warning: Color,
    /// Dims the app behind dialogs.
    pub scrim: Color,
    /// Terminal colours.
    pub terminal: TerminalPalette,
    /// UI text size.
    pub font_size: f32,
    /// Small UI text size.
    pub font_size_small: f32,
    /// Terminal text size.
    pub terminal_font_size: f32,
    /// Corner radius of controls.
    pub radius: f32,
}

/// The 16 ANSI colours plus terminal defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalPalette {
    /// Default foreground.
    pub foreground: Color,
    /// Default background.
    pub background: Color,
    /// Cursor.
    pub cursor: Color,
    /// Selection highlight.
    pub selection: Color,
    /// ANSI 0–15.
    pub ansi: [Color; 16],
}

impl TerminalPalette {
    /// Resolves a 256-colour palette index.
    pub fn indexed(&self, index: u8) -> Color {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let i = index - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Color::rgb8(level(i / 36), level((i / 6) % 6), level(i % 6))
            }
            232..=255 => {
                let v = 8 + (index - 232) * 10;
                Color::rgb8(v, v, v)
            }
        }
    }
}

impl Theme {
    /// The default dark theme.
    pub fn dark() -> Self {
        Self {
            background: Color::hex(0x15161a),
            sidebar: Color::hex(0x1b1c21),
            surface: Color::hex(0x23252b),
            hover: Color::hex(0x2c2e35),
            selected: Color::hex(0x33363f),
            border: Color::hex(0x2f3138),
            text: Color::hex(0xe6e7ea),
            text_muted: Color::hex(0x8f939c),
            accent: Color::hex(0xc8553d),
            on_accent: Color::hex(0xffffff),
            danger: Color::hex(0xe5484d),
            success: Color::hex(0x46a758),
            warning: Color::hex(0xe2a336),
            scrim: Color::hex(0x000000).with_alpha(0.55),
            terminal: TerminalPalette {
                foreground: Color::hex(0xd8d9dc),
                background: Color::hex(0x15161a),
                cursor: Color::hex(0xe6e7ea),
                selection: Color::hex(0x3b5070),
                ansi: [
                    Color::hex(0x1d1f21),
                    Color::hex(0xcc6666),
                    Color::hex(0xb5bd68),
                    Color::hex(0xf0c674),
                    Color::hex(0x81a2be),
                    Color::hex(0xb294bb),
                    Color::hex(0x8abeb7),
                    Color::hex(0xc5c8c6),
                    Color::hex(0x666666),
                    Color::hex(0xd54e53),
                    Color::hex(0xb9ca4a),
                    Color::hex(0xe7c547),
                    Color::hex(0x7aa6da),
                    Color::hex(0xc397d8),
                    Color::hex(0x70c0b1),
                    Color::hex(0xeaeaea),
                ],
            },
            font_size: 13.0,
            font_size_small: 11.5,
            terminal_font_size: 13.0,
            radius: 6.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
