//! Glyph atlas: rasterized glyphs packed into one RGBA texture with a shelf packer.
//!
//! Masks are stored as white with alpha, colour glyphs (emoji) as-is. When the atlas
//! fills up it is cleared and refilled on demand. That's rare, and much simpler than
//! eviction.

use std::collections::HashMap;

use cosmic_text::{CacheKey, SwashCache, SwashContent};
use urushi_ui::{ImageData, TextSystem};

/// Atlas edge length in texels.
pub(crate) const ATLAS_SIZE: u32 = 2048;

/// Where a glyph lives in the atlas, and how to place it relative to its pen position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasGlyph {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) is_color: bool,
}

/// Shelf packer: fills rows left to right, starting a new row when one is full.
#[derive(Debug, Default)]
struct Shelves {
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl Shelves {
    fn insert(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        const PAD: u32 = 1;
        if width + PAD > ATLAS_SIZE || height + PAD > ATLAS_SIZE {
            return None;
        }
        if self.cursor_x + width + PAD > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + PAD;
            self.row_height = 0;
        }
        if self.cursor_y + height + PAD > ATLAS_SIZE {
            return None;
        }
        let at = (self.cursor_x, self.cursor_y);
        self.cursor_x += width + PAD;
        self.row_height = self.row_height.max(height);
        Some(at)
    }
}

/// An upload the renderer must copy into the atlas texture.
pub(crate) struct Upload {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// CPU side of the atlas.
pub(crate) struct Atlas {
    swash: SwashCache,
    shelves: Shelves,
    glyphs: HashMap<CacheKey, Option<AtlasGlyph>>,
    /// Bitmaps by (image id, width, height): each size drawn is scaled once.
    images: HashMap<(u64, u32, u32), AtlasGlyph>,
    pub(crate) pending: Vec<Upload>,
}

/// The atlas ran out of space. Call [`Atlas::clear`] and render the frame again.
#[derive(Debug)]
pub(crate) struct AtlasFull;

impl Atlas {
    pub(crate) fn new() -> Self {
        Self {
            swash: SwashCache::new(),
            shelves: Shelves::default(),
            glyphs: HashMap::new(),
            images: HashMap::new(),
            pending: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.shelves = Shelves::default();
        self.glyphs.clear();
        self.images.clear();
        self.pending.clear();
    }

    /// `image` scaled to `width` × `height` pixels, packed as a colour entry.
    pub(crate) fn image(
        &mut self,
        image: &ImageData,
        width: u32,
        height: u32,
    ) -> Result<AtlasGlyph, AtlasFull> {
        let key = (image.id, width, height);
        if let Some(entry) = self.images.get(&key) {
            return Ok(*entry);
        }
        let (x, y) = self.shelves.insert(width, height).ok_or(AtlasFull)?;
        self.pending.push(Upload {
            x,
            y,
            width,
            height,
            rgba: crate::image::resample(image, width, height),
        });
        let entry = AtlasGlyph {
            x,
            y,
            width,
            height,
            left: 0,
            top: 0,
            is_color: true,
        };
        self.images.insert(key, entry);
        Ok(entry)
    }

    /// The glyph's atlas entry, rasterizing and queueing an upload if it is new. `None`
    /// for glyphs with no pixels (spaces).
    pub(crate) fn get(
        &mut self,
        text: &mut TextSystem,
        key: CacheKey,
    ) -> Result<Option<AtlasGlyph>, AtlasFull> {
        if let Some(entry) = self.glyphs.get(&key) {
            return Ok(*entry);
        }
        let Some(image) = self.swash.get_image_uncached(text.fonts(), key) else {
            self.glyphs.insert(key, None);
            return Ok(None);
        };
        let (width, height) = (image.placement.width, image.placement.height);
        if width == 0 || height == 0 {
            self.glyphs.insert(key, None);
            return Ok(None);
        }
        let (x, y) = self.shelves.insert(width, height).ok_or(AtlasFull)?;
        let (rgba, is_color) = match image.content {
            SwashContent::Mask => (
                image
                    .data
                    .iter()
                    .flat_map(|a| [255, 255, 255, *a])
                    .collect(),
                false,
            ),
            SwashContent::SubpixelMask => (
                image
                    .data
                    .chunks_exact(4)
                    .flat_map(|p| {
                        let a =
                            u8::try_from((u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3)
                                .unwrap_or(255);
                        [255, 255, 255, a]
                    })
                    .collect(),
                false,
            ),
            SwashContent::Color => (image.data.clone(), true),
        };
        self.pending.push(Upload {
            x,
            y,
            width,
            height,
            rgba,
        });
        let glyph = AtlasGlyph {
            x,
            y,
            width,
            height,
            left: image.placement.left,
            top: image.placement.top,
            is_color,
        };
        self.glyphs.insert(key, Some(glyph));
        Ok(Some(glyph))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_pack_rows_and_report_full() {
        let mut s = Shelves::default();
        assert_eq!(s.insert(10, 10), Some((0, 0)));
        assert_eq!(s.insert(10, 20), Some((11, 0)));
        assert_eq!(
            s.insert(ATLAS_SIZE - 5, 5),
            Some((0, 21)),
            "wraps to a new row below the tallest glyph"
        );
        assert_eq!(s.insert(ATLAS_SIZE + 1, 1), None);
        let mut full = Shelves::default();
        let mut count = 0;
        while full.insert(500, 500).is_some() {
            count += 1;
        }
        assert_eq!(count, 16);
    }
}
