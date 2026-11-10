//! Design tokens. Widgets and screens read every colour and size from a [`Theme`], so
//! restyling (or adding a light theme) never touches layout code.
//!
//! The dark theme is frosted glass: a near-black window with a few large, soft colour
//! glows behind everything, translucent panels over them, and overlays (dialogs,
//! menus, the task board) that blur what they cover.

use crate::color::Color;

/// A large, very soft circle of colour behind the whole window. Positions and sizes
/// are fractions of the window, so the backdrop scales with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// Centre, as fractions of the window's width and height.
    pub center: (f32, f32),
    /// Radius, as a fraction of the window's longer side.
    pub radius: f32,
    /// Colour at the centre; it fades out towards the edge.
    pub color: Color,
}

/// Colours, sizes and the terminal palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Window background, under the glows.
    pub background: Color,
    /// The soft colour glows behind everything.
    pub glows: Vec<Glow>,
    /// Glass panels (sidebar, tab bar, status bar): the fill at the top…
    pub glass: Color,
    /// …fading to this at the bottom.
    pub glass_low: Color,
    /// Glass over busy content (dialogs, menus, the task board), on a blurred backdrop.
    pub glass_strong: Color,
    /// The bright hairline around glass.
    pub glass_border: Color,
    /// Recessed inputs (text fields, selectors).
    pub well: Color,
    /// Drop shadows under floating glass.
    pub shadow: Color,
    /// How far overlays blur what's under them, in logical pixels.
    pub blur: f32,
    /// Raised items on glass: cards, secondary buttons, badges.
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
    /// Label colours for cards, columns and tags: red, orange, yellow, green, teal,
    /// blue, purple, pink, gray (the order of `urushi_core::LabelColor::ALL`).
    pub labels: [Color; 9],
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
    /// Corner radius of panels and dialogs.
    pub radius_large: f32,
}

/// The 16 ANSI colours plus terminal defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalPalette {
    /// Default foreground.
    pub foreground: Color,
    /// Default background. Translucent, so the window's glows tint the panes; cells
    /// that use it as a colour (inverse video) take it opaque.
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
        let white = |a: f32| Color::hex(0xffffff).with_alpha(a);
        let black = |a: f32| Color::hex(0x000000).with_alpha(a);
        Self {
            background: Color::hex(0x0a0b10),
            glows: vec![
                // Urushi red, high on the left, behind the sidebar.
                Glow {
                    center: (0.05, 0.02),
                    radius: 0.55,
                    color: Color::hex(0xc8553d).with_alpha(0.30),
                },
                // Indigo, low on the right, behind the terminals.
                Glow {
                    center: (0.92, 1.0),
                    radius: 0.65,
                    color: Color::hex(0x4a52d6).with_alpha(0.26),
                },
                // A faint teal in between.
                Glow {
                    center: (0.55, 0.35),
                    radius: 0.42,
                    color: Color::hex(0x1e9a94).with_alpha(0.12),
                },
            ],
            glass: Color::hex(0x1f2230).with_alpha(0.55),
            glass_low: Color::hex(0x151722).with_alpha(0.50),
            glass_strong: Color::hex(0x14161e).with_alpha(0.78),
            glass_border: white(0.09),
            well: black(0.28),
            shadow: black(0.55),
            blur: 28.0,
            surface: white(0.055),
            hover: white(0.07),
            selected: white(0.11),
            border: white(0.08),
            text: Color::hex(0xe8e9ee),
            text_muted: Color::hex(0x959aa6),
            accent: Color::hex(0xc8553d),
            on_accent: Color::hex(0xffffff),
            danger: Color::hex(0xe5484d),
            success: Color::hex(0x46a758),
            warning: Color::hex(0xe2a336),
            scrim: black(0.32),
            labels: [
                Color::hex(0xe5534b),
                Color::hex(0xe8853d),
                Color::hex(0xd9b73a),
                Color::hex(0x57ab5a),
                Color::hex(0x39a9a0),
                Color::hex(0x4d8fe0),
                Color::hex(0x9a72d8),
                Color::hex(0xd8679f),
                Color::hex(0x868a93),
            ],
            terminal: TerminalPalette {
                foreground: Color::hex(0xd8d9dc),
                background: Color::hex(0x0b0c12).with_alpha(0.76),
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
            radius_large: 12.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
