//! `urushi settings set colors|logo`: the app's look (protocol 1.13), shared by every
//! client. `colors` takes a preset (urushi, ocean, forest, dusk, ember, mono) or five
//! `#rrggbb` colors (background, three glows, accent) separated by commas; `logo` takes
//! original, accent, mono or hidden.

use anyhow::{Result, bail};
use urushi_client::{Client, v1};
use urushi_core::{Appearance, LogoStyle, Rgb, Scheme, SchemeColors};
use urushi_proto::convert::{appearance_from_proto, appearance_to_proto};

/// Whether `key` is one of the look's settings.
pub(crate) fn is_look(key: &str) -> bool {
    matches!(key, "colors" | "logo")
}

/// The look with `key` set to `value`.
fn changed(mut look: Appearance, key: &str, value: &str) -> Result<Appearance> {
    let value = value.trim().to_lowercase();
    if key == "logo" {
        look.logo = LogoStyle::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(&value))
            .ok_or_else(|| anyhow::anyhow!("expected original, accent, mono or hidden"))?;
        return Ok(look);
    }
    if let Some(preset) = Scheme::PRESETS
        .into_iter()
        .find(|s| s.name().eq_ignore_ascii_case(&value))
    {
        look.scheme = preset;
        return Ok(look);
    }
    let colors: Vec<Rgb> = value.split(',').filter_map(Rgb::parse).collect();
    let [background, g1, g2, g3, accent] = colors.as_slice() else {
        bail!("expected a preset name or five #rrggbb colors (background, 3 glows, accent)");
    };
    look.scheme = Scheme::Custom(SchemeColors {
        background: *background,
        glows: [*g1, *g2, *g3],
        accent: *accent,
    });
    Ok(look)
}

/// Sets one of the look's settings, keeping the rest of it.
pub(crate) async fn set(client: &Client, key: &str, value: &str) -> Result<v1::Settings> {
    let now = client.settings().await?;
    let look = changed(appearance_from_proto(now.appearance.as_ref()), key, value)?;
    let patch = v1::SettingsPatch {
        appearance: Some(appearance_to_proto(look)),
        ..Default::default()
    };
    Ok(client.update_settings(patch).await?)
}

/// The look as `settings get` prints it.
pub(crate) fn describe(settings: &v1::Settings) -> (String, String) {
    let look = appearance_from_proto(settings.appearance.as_ref());
    let colors = match look.scheme {
        Scheme::Custom(c) => format!(
            "{},{},{},{},{}",
            c.background.hex(),
            c.glows[0].hex(),
            c.glows[1].hex(),
            c.glows[2].hex(),
            c.accent.hex()
        ),
        preset => preset.name().to_lowercase(),
    };
    (colors, look.logo.name().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_custom_colors_and_logos_are_read() {
        let base = Appearance::default();
        assert_eq!(
            changed(base, "colors", "Ocean").unwrap().scheme,
            Scheme::Ocean
        );
        let custom = changed(base, "colors", "#000000,#111111,#222222,#333333,#ff0000").unwrap();
        assert_eq!(custom.scheme.colors().accent, Rgb::new(0xff0000));
        assert!(changed(base, "colors", "#000000,#111111").is_err());
        let hidden = changed(custom, "logo", "hidden").unwrap();
        assert_eq!(
            (hidden.logo, hidden.scheme),
            (LogoStyle::Hidden, custom.scheme)
        );
        assert!(changed(base, "logo", "sparkly").is_err());
    }
}
