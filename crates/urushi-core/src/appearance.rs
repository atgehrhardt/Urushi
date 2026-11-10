//! How the app looks, kept with the daemon's settings so every client shows the same:
//! the colors of the backdrop's glows behind the frosted glass, the background under
//! them and the accent (a preset scheme, or the user's own), and how the logo is drawn.
//!
//! Colors are plain `0xRRGGBB` values ([`Rgb`]); clients turn them into a theme.

use serde::{Deserialize, Serialize};

/// A color as `0xRRGGBB`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rgb(u32);

impl Rgb {
    /// A color from `0xRRGGBB` (higher bits are dropped).
    pub const fn new(rgb: u32) -> Self {
        Self(rgb & 0x00ff_ffff)
    }

    /// The color as `0xRRGGBB`.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Reads `#rrggbb`, `rrggbb` or `#rgb` (ignoring case and spaces).
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        let full = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect(),
            6 => hex.to_owned(),
            _ => return None,
        };
        u32::from_str_radix(&full, 16).ok().map(Self::new)
    }

    /// The color as `#rrggbb`.
    pub fn hex(self) -> String {
        format!("#{:06x}", self.0)
    }
}

/// The colors of a scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeColors {
    /// The window's fill, under the glows.
    pub background: Rgb,
    /// The three soft glows (top left, bottom right, middle).
    pub glows: [Rgb; 3],
    /// Buttons, focus, the selection.
    pub accent: Rgb,
}

/// A preset color scheme, or the user's own colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "lowercase")]
pub enum Scheme {
    /// Lacquer red and indigo with a touch of teal (the original).
    #[default]
    Urushi,
    /// Deep blues and cyan.
    Ocean,
    /// Greens and moss.
    Forest,
    /// Violet and rose.
    Dusk,
    /// Amber and copper.
    Ember,
    /// Greys, with a quiet accent.
    Mono,
    /// The user's own colors.
    Custom(SchemeColors),
}

impl Scheme {
    /// The presets, in the order they are offered.
    pub const PRESETS: [Self; 6] = [
        Self::Urushi,
        Self::Ocean,
        Self::Forest,
        Self::Dusk,
        Self::Ember,
        Self::Mono,
    ];

    /// Its name, as offered.
    pub fn name(self) -> &'static str {
        match self {
            Self::Urushi => "Urushi",
            Self::Ocean => "Ocean",
            Self::Forest => "Forest",
            Self::Dusk => "Dusk",
            Self::Ember => "Ember",
            Self::Mono => "Mono",
            Self::Custom(_) => "Custom",
        }
    }

    /// Its colors.
    pub fn colors(self) -> SchemeColors {
        let c = |background, glows: [u32; 3], accent| SchemeColors {
            background: Rgb::new(background),
            glows: glows.map(Rgb::new),
            accent: Rgb::new(accent),
        };
        match self {
            Self::Urushi => c(0x0a0b10, [0xc8553d, 0x4a52d6, 0x1e9a94], 0xc8553d),
            Self::Ocean => c(0x070b12, [0x1f6fd1, 0x0f9fb5, 0x3d4fd6], 0x2f86e0),
            Self::Forest => c(0x080c09, [0x3f9a4f, 0x1f6f5c, 0xa3b23a], 0x4caf63),
            Self::Dusk => c(0x0c0910, [0x9b4fd6, 0xd64f8c, 0x4f5fd6], 0xa45be0),
            Self::Ember => c(0x0d0907, [0xe0832b, 0xc8553d, 0xd6a93a], 0xe08a2f),
            Self::Mono => c(0x0b0b0c, [0x5c5f6b, 0x3c3f4a, 0x8a8d96], 0x8f95a3),
            Self::Custom(colors) => colors,
        }
    }
}

/// How the logo beside the app's name is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogoStyle {
    /// In its own colors.
    #[default]
    Original,
    /// In the scheme's accent.
    Accent,
    /// In the text color.
    Mono,
    /// Not shown.
    Hidden,
}

impl LogoStyle {
    /// Every style, in the order offered.
    pub const ALL: [Self; 4] = [Self::Original, Self::Accent, Self::Mono, Self::Hidden];

    /// Its name, as offered.
    pub fn name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Accent => "Accent",
            Self::Mono => "Mono",
            Self::Hidden => "Hidden",
        }
    }
}

/// How the app looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// The backdrop's colors and the accent.
    pub scheme: Scheme,
    /// How the logo is drawn.
    pub logo: LogoStyle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_read_and_write_as_hex() {
        assert_eq!(Rgb::parse("#C8553D"), Some(Rgb::new(0xc8553d)));
        assert_eq!(Rgb::parse(" 0a0b10 "), Some(Rgb::new(0x0a0b10)));
        assert_eq!(Rgb::parse("#fa0"), Some(Rgb::new(0xffaa00)));
        for bad in ["", "#12345", "red", "#gg0000"] {
            assert_eq!(Rgb::parse(bad), None, "{bad}");
        }
        assert_eq!(Rgb::new(0x1f22_3000).hex(), "#223000");
    }

    #[test]
    fn the_original_scheme_is_the_default_and_custom_keeps_its_colors() {
        assert_eq!(
            Appearance::default().scheme.colors().accent,
            Rgb::new(0xc8553d)
        );
        let mine = SchemeColors {
            background: Rgb::new(1),
            glows: [Rgb::new(2), Rgb::new(3), Rgb::new(4)],
            accent: Rgb::new(5),
        };
        assert_eq!(Scheme::Custom(mine).colors(), mine);
        assert!(Scheme::PRESETS.iter().all(|s| s.name() != "Custom"));
    }

    #[test]
    fn appearances_round_trip_and_old_documents_load() {
        let a = Appearance {
            scheme: Scheme::Custom(Scheme::Ocean.colors()),
            logo: LogoStyle::Hidden,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Appearance>(&json).unwrap(), a);
        assert_eq!(
            serde_json::from_str::<Appearance>("{}").unwrap(),
            Appearance::default()
        );
    }
}
