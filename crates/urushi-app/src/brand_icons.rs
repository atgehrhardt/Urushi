//! The Jira and GitHub marks a thread card shows for its linked issue and pull request
//! (ADR 0036). They are Simple Icons' glyphs (`assets/brands/`), pre-rendered by
//! `scripts/render-icons.swift` as white masks, and tinted here to a theme color so
//! they sit with the card's text in every appearance.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use urushi_ui::{Color, ImageData};

use crate::appearance::{byte, packed};

/// The marks' size in pixels.
const SIZE: u32 = 48;
/// The renderer caches bitmaps by id: brand marks set this bit, which neither the logo
/// (small ids) nor attachment thumbnails (bit 63) use.
const BRAND_BIT: u64 = 1 << 56;

/// The marks made so far, by brand and tint.
type Marks = HashMap<(Brand, u32), Arc<ImageData>>;

/// A brand whose mark a card can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Brand {
    /// Jira: a linked issue.
    Jira,
    /// GitHub: a linked pull request.
    GitHub,
}

impl Brand {
    fn mask(self) -> &'static [u8] {
        match self {
            Self::Jira => include_bytes!("../../../assets/brands/jira-48.rgba"),
            Self::GitHub => include_bytes!("../../../assets/brands/github-48.rgba"),
        }
    }
}

/// `brand`'s mark in `color` (`None` only if the bundled bitmap were malformed).
pub(crate) fn mark(brand: Brand, color: Color) -> Option<Arc<ImageData>> {
    static CACHE: LazyLock<Mutex<Marks>> = LazyLock::new(Mutex::default);
    let tint = packed(color);
    let mut cache = CACHE.lock().ok()?;
    if let Some(image) = cache.get(&(brand, tint)) {
        return Some(image.clone());
    }
    let (r, g, b) = (byte(color.r), byte(color.g), byte(color.b));
    let pixels = brand
        .mask()
        .chunks_exact(4)
        .flat_map(|p| [r, g, b, p[3]])
        .collect();
    let id = BRAND_BIT | (brand as u64) << 40 | u64::from(tint);
    let image = Arc::new(ImageData::new(id, SIZE, SIZE, pixels)?);
    // A few theme colors at most; drop them all if themes are cycled through a lot.
    if cache.len() >= 16 {
        cache.clear();
    }
    cache.insert((brand, tint), image.clone());
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_tinted_masks_with_their_own_ids() {
        let red = Color::hex(0xff0000);
        let jira = mark(Brand::Jira, red).unwrap();
        let github = mark(Brand::GitHub, red).unwrap();
        assert_eq!((jira.width, jira.height), (SIZE, SIZE));
        assert_ne!(jira.id, github.id);
        assert_ne!(jira.id, mark(Brand::Jira, Color::hex(0x00ff00)).unwrap().id);
        assert!(
            Arc::ptr_eq(&jira, &mark(Brand::Jira, red).unwrap()),
            "cached"
        );
        let opaque = jira.rgba.chunks_exact(4).find(|p| p[3] == 255).unwrap();
        assert_eq!(&opaque[..3], &[255, 0, 0]);
    }
}
