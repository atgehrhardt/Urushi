//! The app's appearance: core ⇄ wire (since 1.13). Unknown schemes and logo styles from
//! a newer peer read as the original; custom colors missing a glow get the original's.

use urushi_core::{Appearance, LogoStyle, Rgb, Scheme, SchemeColors};

use crate::v1;

/// Core appearance → wire.
pub fn appearance_to_proto(a: Appearance) -> v1::Appearance {
    use v1::ColorScheme as C;
    let scheme = match a.scheme {
        Scheme::Urushi => C::Urushi,
        Scheme::Ocean => C::Ocean,
        Scheme::Forest => C::Forest,
        Scheme::Dusk => C::Dusk,
        Scheme::Ember => C::Ember,
        Scheme::Mono => C::Mono,
        Scheme::Custom(_) => C::Custom,
    };
    let custom = match a.scheme {
        Scheme::Custom(colors) => Some(v1::SchemeColors {
            background: colors.background.value(),
            glows: colors.glows.iter().map(|g| g.value()).collect(),
            accent: colors.accent.value(),
        }),
        _ => None,
    };
    let logo = match a.logo {
        LogoStyle::Original => v1::LogoStyle::Original,
        LogoStyle::Accent => v1::LogoStyle::Accent,
        LogoStyle::Mono => v1::LogoStyle::Mono,
        LogoStyle::Hidden => v1::LogoStyle::Hidden,
    };
    v1::Appearance {
        scheme: scheme.into(),
        custom,
        logo: logo.into(),
    }
}

/// Wire appearance → core (unset: the original look).
pub fn appearance_from_proto(a: Option<&v1::Appearance>) -> Appearance {
    use v1::ColorScheme as C;
    let Some(a) = a else {
        return Appearance::default();
    };
    let scheme = match v1::ColorScheme::try_from(a.scheme).unwrap_or(C::Unspecified) {
        C::Unspecified | C::Urushi => Scheme::Urushi,
        C::Ocean => Scheme::Ocean,
        C::Forest => Scheme::Forest,
        C::Dusk => Scheme::Dusk,
        C::Ember => Scheme::Ember,
        C::Mono => Scheme::Mono,
        C::Custom => {
            let original = Scheme::Urushi.colors();
            let custom = a.custom.clone().unwrap_or_default();
            let glow = |i: usize| {
                custom
                    .glows
                    .get(i)
                    .map_or(original.glows[i], |g| Rgb::new(*g))
            };
            Scheme::Custom(SchemeColors {
                background: Rgb::new(custom.background),
                glows: [glow(0), glow(1), glow(2)],
                accent: Rgb::new(custom.accent),
            })
        }
    };
    let logo = match v1::LogoStyle::try_from(a.logo).unwrap_or(v1::LogoStyle::Unspecified) {
        v1::LogoStyle::Unspecified | v1::LogoStyle::Original => LogoStyle::Original,
        v1::LogoStyle::Accent => LogoStyle::Accent,
        v1::LogoStyle::Mono => LogoStyle::Mono,
        v1::LogoStyle::Hidden => LogoStyle::Hidden,
    };
    Appearance { scheme, logo }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    #[test]
    fn appearances_round_trip() {
        let custom = Scheme::Custom(SchemeColors {
            background: Rgb::new(0x101010),
            glows: [Rgb::new(1), Rgb::new(2), Rgb::new(3)],
            accent: Rgb::new(0xff0000),
        });
        for scheme in Scheme::PRESETS.into_iter().chain([custom]) {
            for logo in LogoStyle::ALL {
                let a = Appearance { scheme, logo };
                let wire = appearance_to_proto(a);
                let decoded = v1::Appearance::decode(wire.encode_to_vec().as_slice()).unwrap();
                assert_eq!(appearance_from_proto(Some(&decoded)), a);
            }
        }
        assert_eq!(appearance_from_proto(None), Appearance::default());
        let odd = v1::Appearance {
            scheme: 99,
            logo: 99,
            custom: None,
        };
        assert_eq!(appearance_from_proto(Some(&odd)), Appearance::default());
    }
}
