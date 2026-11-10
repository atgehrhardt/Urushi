//! How the app looks (`Settings.appearance`, protocol 1.13): the theme for a color
//! scheme, the logo drawn in a style, and Settings' Appearance section while it is
//! being changed (shown live, kept only on Save).
//!
//! The appearance is the daemon's, like every setting (ADR 0002), so every client
//! shows the same; the runner rebuilds its theme whenever it changes.

use std::sync::{Arc, LazyLock, Mutex};

use urushi_client::v1;
use urushi_core::{Appearance, LogoStyle, Rgb, Scheme, SchemeColors};
use urushi_ui::{Color, ImageData, TextField, Theme};

use crate::state::{AppState, Dialog};

/// The custom colors' names, in the order of [`LookDraft::hex`].
pub const COLOR_NAMES: [&str; 5] = ["Background", "Glow 1", "Glow 2", "Glow 3", "Accent"];

/// The logo's pixels: the mark from the app icon, as raw 128×128 RGBA.
const MARK: &[u8] = include_bytes!("../../../assets/icon/mark-128.rgba");
/// The logo's size in pixels.
const MARK_SIZE: u32 = 128;
/// The renderer caches bitmaps by id; the logo's ids are small and its own.
const LOGO_ID: u64 = 1;

fn color(rgb: Rgb) -> Color {
    Color::hex(rgb.value())
}

/// The theme for an appearance.
pub fn theme_for(appearance: Appearance) -> Theme {
    let c = appearance.scheme.colors();
    Theme::dark().with_scheme(color(c.background), c.glows.map(color), color(c.accent))
}

/// The logo in `style` for `theme` (`None` when hidden, or unreadable).
pub fn logo(style: LogoStyle, theme: &Theme) -> Option<Arc<ImageData>> {
    /// The last logo made: its style, tint and bitmap.
    type Made = Option<(LogoStyle, u32, Arc<ImageData>)>;
    static CACHE: LazyLock<Mutex<Made>> = LazyLock::new(Mutex::default);
    let tint = match style {
        LogoStyle::Hidden => return None,
        LogoStyle::Original => None,
        LogoStyle::Accent => Some((theme.accent, Color::hex(0xffffff))),
        LogoStyle::Mono => Some((theme.text_muted, theme.text)),
    };
    let key = tint.map_or(0, |(c, _)| packed(c));
    let mut cache = CACHE.lock().ok()?;
    if let Some((s, k, image)) = cache.as_ref()
        && *s == style
        && *k == key
    {
        return Some(image.clone());
    }
    let pixels = match tint {
        None => MARK.to_vec(),
        Some((dark, light)) => tinted(dark, light),
    };
    // A new bitmap id per look, so the renderer doesn't draw a cached one.
    let id = LOGO_ID | (u64::from(key) << 8) | (style as u64) << 40;
    let image = Arc::new(ImageData::new(id, MARK_SIZE, MARK_SIZE, pixels)?);
    *cache = Some((style, key, image.clone()));
    Some(image)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "components are clamped to 0–255"
)]
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn packed(c: Color) -> u32 {
    u32::from(byte(c.r)) << 16 | u32::from(byte(c.g)) << 8 | u32::from(byte(c.b))
}

/// The mark recolored: its colored parts in `dark`, its light strokes in `light`, each
/// pixel by how light it was; its shape (alpha) kept.
fn tinted(dark: Color, light: Color) -> Vec<u8> {
    MARK.chunks_exact(4)
        .flat_map(|p| {
            let whiteness = f32::from(p[0].min(p[1]).min(p[2])) / 255.0;
            let c = dark.mix(light, whiteness);
            [byte(c.r), byte(c.g), byte(c.b), p[3]]
        })
        .collect()
}

/// Settings' Appearance section while it is changed: a preset or custom colors (typed
/// as `#rrggbb`), and the logo's style.
#[derive(Clone, Debug, PartialEq)]
pub struct LookDraft {
    /// The preset chosen, unless `custom`.
    pub preset: Scheme,
    /// The custom colors are chosen.
    pub custom: bool,
    /// The custom colors as typed ([`COLOR_NAMES`]).
    pub hex: [TextField; 5],
    /// The logo's style.
    pub logo: LogoStyle,
}

impl LookDraft {
    /// The draft of an appearance.
    pub fn of(a: Appearance) -> Self {
        let c = a.scheme.colors();
        let colors = [c.background, c.glows[0], c.glows[1], c.glows[2], c.accent];
        Self {
            preset: match a.scheme {
                Scheme::Custom(_) => Scheme::Urushi,
                preset => preset,
            },
            custom: matches!(a.scheme, Scheme::Custom(_)),
            hex: colors.map(|c| TextField::new(c.hex())),
            logo: a.logo,
        }
    }

    /// The appearance it shows: typed colors that don't read as one keep the preset's.
    pub fn appearance(&self) -> Appearance {
        let scheme = if self.custom {
            let base = self.preset.colors();
            let fallback = [
                base.background,
                base.glows[0],
                base.glows[1],
                base.glows[2],
                base.accent,
            ];
            let c: Vec<Rgb> = self
                .hex
                .iter()
                .zip(fallback)
                .map(|(t, f)| Rgb::parse(t.text()).unwrap_or(f))
                .collect();
            Scheme::Custom(SchemeColors {
                background: c[0],
                glows: [c[1], c[2], c[3]],
                accent: c[4],
            })
        } else {
            self.preset
        };
        Appearance {
            scheme,
            logo: self.logo,
        }
    }

    /// Chooses a preset; its colors become the custom ones to start from.
    pub fn choose(&mut self, preset: Option<Scheme>) {
        match preset {
            Some(p) => {
                self.preset = p;
                self.custom = false;
                let c = p.colors();
                let colors = [c.background, c.glows[0], c.glows[1], c.glows[2], c.accent];
                self.hex = colors.map(|c| TextField::new(c.hex()));
            }
            None => self.custom = true,
        }
    }
}

impl AppState {
    /// How the app looks now: Settings' draft while it is open, else the daemon's.
    pub fn appearance(&self) -> Appearance {
        if let Some(Dialog::Settings { look, .. }) = &self.dialog {
            return look.appearance();
        }
        saved(self.settings.as_ref())
    }
}

/// The appearance in the daemon's settings (the original look before they arrive).
pub fn saved(settings: Option<&v1::Settings>) -> Appearance {
    urushi_proto::convert::appearance_from_proto(settings.and_then(|s| s.appearance.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_colors_the_backdrop_and_accent() {
        let ocean = theme_for(Appearance {
            scheme: Scheme::Ocean,
            logo: LogoStyle::Original,
        });
        assert_eq!(ocean.accent, Color::hex(0x2f86e0));
        assert_eq!(ocean.background, Color::hex(0x070b12));
        assert_eq!(ocean.glows.len(), 3);
        assert!((ocean.glows[0].color.a - Theme::dark().glows[0].color.a).abs() < 1e-6);
        assert_eq!(
            theme_for(Appearance::default()).accent,
            Theme::dark().accent
        );
    }

    #[test]
    fn the_logo_is_tinted_hidden_or_as_it_is() {
        let theme = Theme::dark();
        assert!(logo(LogoStyle::Hidden, &theme).is_none());
        let original = logo(LogoStyle::Original, &theme).unwrap();
        let accent = logo(LogoStyle::Accent, &theme).unwrap();
        assert_ne!(
            original.id, accent.id,
            "the renderer must not reuse the bitmap"
        );
        let tinted = tinted(Color::hex(0x00ff00), Color::hex(0xffffff));
        assert_eq!(tinted.len(), MARK.len());
        // Shapes are kept.
        assert!(
            tinted
                .chunks(4)
                .zip(MARK.chunks(4))
                .all(|(t, m)| t[3] == m[3])
        );
    }

    #[test]
    fn a_draft_shows_typed_colors_and_keeps_the_preset_for_odd_ones() {
        let mut d = LookDraft::of(Appearance::default());
        assert_eq!(d.appearance(), Appearance::default());
        d.choose(Some(Scheme::Forest));
        assert_eq!(d.appearance().scheme, Scheme::Forest);
        d.choose(None);
        d.hex[4] = TextField::new("#ff0000".to_owned());
        d.hex[0] = TextField::new("not a color".to_owned());
        let c = d.appearance().scheme.colors();
        assert_eq!(c.accent, Rgb::new(0xff0000));
        assert_eq!(c.background, Scheme::Forest.colors().background);
    }
}
