//! Fonts, text measurement and shaping, shared by layout (measuring) and the renderer
//! (rasterizing).
//!
//! The UI font (Inter) and terminal font (`JetBrains Mono`) are bundled, both OFL-1.1, so
//! text looks the same on every platform. System fonts are added as fallbacks for
//! scripts and emoji the bundled fonts don't cover.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, fontdb};

use crate::scene::Font;

const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf");
const MONO_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf");
const MONO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-BoldItalic.ttf");

const UI_FAMILY: &str = "Inter";
const MONO_FAMILY: &str = "JetBrains Mono";

/// Size of one terminal cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    /// Cell width (the monospace advance).
    pub width: f32,
    /// Cell height (the line height).
    pub height: f32,
}

/// Fonts plus measurement caches.
pub struct TextSystem {
    fonts: FontSystem,
    widths: HashMap<(String, Font, u32), f32>,
    cells: HashMap<u32, CellMetrics>,
}

impl TextSystem {
    /// Bundled fonts plus system fonts as fallbacks.
    pub fn new() -> Self {
        let mut db = bundled_db();
        db.load_system_fonts();
        Self::with_db(db)
    }

    /// Bundled fonts only. Deterministic across machines, for tests.
    pub fn bundled_only() -> Self {
        Self::with_db(bundled_db())
    }

    fn with_db(db: fontdb::Database) -> Self {
        Self {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), db),
            widths: HashMap::new(),
            cells: HashMap::new(),
        }
    }

    /// The underlying font system (for the renderer).
    pub fn fonts(&mut self) -> &mut FontSystem {
        &mut self.fonts
    }

    /// cosmic-text attributes for `font`.
    pub fn attrs(font: Font) -> Attrs<'static> {
        let attrs = Attrs::new();
        match font {
            Font::Ui => attrs.family(Family::Name(UI_FAMILY)).weight(Weight::NORMAL),
            Font::UiMedium => attrs.family(Family::Name(UI_FAMILY)).weight(Weight::MEDIUM),
            Font::UiSemibold => attrs
                .family(Family::Name(UI_FAMILY))
                .weight(Weight::SEMIBOLD),
            Font::Mono { bold, italic } => attrs
                .family(Family::Name(MONO_FAMILY))
                .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
                .style(if italic { Style::Italic } else { Style::Normal }),
        }
    }

    /// Height of a line of text at `size`.
    pub fn line_height(size: f32) -> f32 {
        (size * 1.35).round()
    }

    /// Shapes one line of `text` (no wrapping).
    pub fn shape(&mut self, text: &str, font: Font, size: f32) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, Self::line_height(size)));
        buffer.set_size(None, None);
        buffer.set_text(text, &Self::attrs(font), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    /// Width of one line of text.
    pub fn measure(&mut self, text: &str, font: Font, size: f32) -> f32 {
        let key = (text.to_owned(), font, size.to_bits());
        if let Some(width) = self.widths.get(&key) {
            return *width;
        }
        let width = self
            .shape(text, font, size)
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        if self.widths.len() > 10_000 {
            self.widths.clear();
        }
        self.widths.insert(key, width);
        width
    }

    /// `text`, shortened with an ellipsis if it is wider than `max_width`.
    pub fn truncate(&mut self, text: &str, font: Font, size: f32, max_width: f32) -> String {
        if self.measure(text, font, size) <= max_width {
            return text.to_owned();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>() + "…";
            if self.measure(&candidate, font, size) <= max_width {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        chars[..low].iter().collect::<String>() + "…"
    }

    /// Terminal cell size at `size`.
    pub fn cell_metrics(&mut self, size: f32) -> CellMetrics {
        if let Some(m) = self.cells.get(&size.to_bits()) {
            return *m;
        }
        // The average advance of a long run is exact for a monospace font.
        let sample = "M".repeat(32);
        #[allow(clippy::cast_precision_loss, reason = "32 is exact")]
        let width = self.measure(&sample, Font::MONO, size) / sample.len() as f32;
        let metrics = CellMetrics {
            width,
            height: Self::line_height(size),
        };
        self.cells.insert(size.to_bits(), metrics);
        metrics
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn bundled_db() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    for data in [
        INTER_REGULAR,
        INTER_MEDIUM,
        INTER_SEMIBOLD,
        MONO_REGULAR,
        MONO_BOLD,
        MONO_ITALIC,
        MONO_BOLD_ITALIC,
    ] {
        db.load_font_data(data.to_vec());
    }
    db.set_sans_serif_family(UI_FAMILY);
    db.set_monospace_family(MONO_FAMILY);
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_truncates() {
        let mut t = TextSystem::bundled_only();
        let short = t.measure("Hi", Font::Ui, 13.0);
        let long = t.measure("Hello, world", Font::Ui, 13.0);
        assert!(short > 0.0 && long > short);
        let cut = t.truncate("Hello, world", Font::Ui, 13.0, long / 2.0);
        assert!(cut.ends_with('…') && t.measure(&cut, Font::Ui, 13.0) <= long / 2.0);
        assert_eq!(t.truncate("Hi", Font::Ui, 13.0, 100.0), "Hi");
    }

    #[test]
    fn monospace_cells() {
        let mut t = TextSystem::bundled_only();
        let m = t.cell_metrics(13.0);
        assert!(m.width > 5.0 && m.width < 10.0, "{m:?}");
        let i = t.measure("iiii", Font::MONO, 13.0);
        let w = t.measure("WWWW", Font::MONO, 13.0);
        assert!((i - w).abs() < 0.01, "monospace");
    }
}
