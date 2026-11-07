//! Colours (straight, non-premultiplied sRGB with alpha, components in 0–1).

/// An sRGB colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha (1 = opaque).
    pub a: f32,
}

impl Color {
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// From `0xRRGGBB`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "each component is masked to 8 bits"
    )]
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb8(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }

    /// From 8-bit components.
    #[allow(clippy::cast_lossless, reason = "`f32::from` is not const")]
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// The same colour with alpha `a`.
    #[must_use]
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Linear blend towards `other` by `t` (0–1).
    #[must_use]
    pub fn mix(self, other: Self, t: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Self {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    /// `#rrggbb` or `#rrggbbaa`, for scene dumps.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0–255 first"
    )]
    pub fn to_hex(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                c(self.r),
                c(self.g),
                c(self.b),
                c(self.a)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_mix() {
        assert_eq!(Color::hex(0x12ab34).to_hex(), "#12ab34");
        assert_eq!(Color::hex(0xffffff).with_alpha(0.5).to_hex(), "#ffffff80");
        assert_eq!(
            Color::hex(0x000000).mix(Color::hex(0xffffff), 0.5).to_hex(),
            "#808080"
        );
    }
}
