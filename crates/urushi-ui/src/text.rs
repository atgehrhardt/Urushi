//! Fonts, text measurement and shaping, shared by layout (measuring) and the renderer
//! (rasterizing).
//!
//! The UI font (Inter) and terminal font (`JetBrains Mono`) are bundled, both OFL-1.1, so
//! text looks the same on every platform. System fonts are added as fallbacks for
//! scripts and emoji the bundled fonts don't cover.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, fontdb};

use crate::scene::Font;
use crate::text_cache::TextCache;

mod wrap;

pub use wrap::Measure;

const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
const INTER_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-Italic.ttf");
const INTER_SEMIBOLD_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/Inter-SemiBoldItalic.ttf");
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
    widths: TextCache<f32>,
    /// How many times text was shaped to be measured (cache misses), for tests.
    shaped: u64,
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
            widths: TextCache::new(WIDTHS_PER_GENERATION),
            shaped: 0,
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
            Font::UiItalic => attrs
                .family(Family::Name(UI_FAMILY))
                .weight(Weight::NORMAL)
                .style(Style::Italic),
            Font::UiSemiboldItalic => attrs
                .family(Family::Name(UI_FAMILY))
                .weight(Weight::SEMIBOLD)
                .style(Style::Italic),
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
        shape_line(&mut self.fonts, text, font, size)
    }

    /// Width of one line of text.
    pub fn measure(&mut self, text: &str, font: Font, size: f32) -> f32 {
        let Self {
            fonts,
            widths,
            shaped,
            ..
        } = self;
        *widths.get_or_insert_with((text, font, size.to_bits()), || {
            *shaped += 1;
            shape_line(fonts, text, font, size)
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0, f32::max)
        })
    }

    /// How many lines of text have been shaped to measure them: what the cache missed.
    pub fn shaped_count(&self) -> u64 {
        self.shaped
    }

    /// `text`, shortened with an ellipsis if it is wider than `max_width`.
    pub fn truncate(&mut self, text: &str, font: Font, size: f32, max_width: f32) -> String {
        let chars: Vec<char> = text.chars().collect();
        // No more characters than this fit, so a long text is never measured whole.
        let most = Self::most_chars(size, max_width);
        if chars.len() <= most && self.measure(text, font, size) <= max_width {
            return text.to_owned();
        }
        let (mut low, mut high) = (0, chars.len().min(most));
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

    /// The most characters of text at `size` that can fit in `width`: no glyph in the
    /// bundled fonts is narrower than a fifth of the size (zero-width ones aside, which
    /// only make text end a little early).
    pub fn most_chars(size: f32, width: f32) -> usize {
        let narrowest = (size * NARROWEST_GLYPH).max(0.5);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative count of characters on one line"
        )]
        let most = (width.max(0.0) / narrowest).ceil() as usize;
        most.saturating_add(1)
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

/// The narrowest glyph of the bundled fonts, as a share of the font size (Inter's
/// thinnest letters and punctuation are about a quarter of it).
const NARROWEST_GLYPH: f32 = 0.2;

/// Entries in each generation of the width cache: enough for every label of a large
/// board, several times over.
const WIDTHS_PER_GENERATION: usize = 20_000;

/// Shapes one line of `text` (no wrapping) with `fonts`.
fn shape_line(fonts: &mut FontSystem, text: &str, font: Font, size: f32) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(size, TextSystem::line_height(size)));
    buffer.set_size(None, None);
    buffer.set_text(text, &TextSystem::attrs(font), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    buffer
}

fn bundled_db() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    for data in [
        INTER_REGULAR,
        INTER_MEDIUM,
        INTER_SEMIBOLD,
        INTER_ITALIC,
        INTER_SEMIBOLD_ITALIC,
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
    fn truncating_a_long_text_measures_only_what_could_fit() {
        let long = "word ".repeat(400);
        let mut t = TextSystem::bundled_only();
        let short = t.truncate(&long, Font::Ui, 13.0, 120.0);
        assert!(
            short.ends_with('…') && short.chars().count() < 60,
            "{short}"
        );
        assert!(t.measure(&short, Font::Ui, 13.0) <= 120.0);
        // A binary search over a bounded prefix, never the whole text.
        assert!(t.shaped_count() < 12, "{}", t.shaped_count());
        let again = t.shaped_count();
        assert_eq!(t.truncate(&long, Font::Ui, 13.0, 120.0), short);
        assert_eq!(t.shaped_count(), again, "a second time is all cache hits");
    }

    #[test]
    fn ui_italics_use_their_own_bundled_faces() {
        let mut t = TextSystem::bundled_only();
        let face = |t: &mut TextSystem, font| {
            let buffer = t.shape("Notes", font, 13.0);
            let run = buffer.layout_runs().next().map(|r| r.glyphs[0].font_id);
            run.and_then(|id| t.fonts().db().face(id).map(|f| (f.style, f.weight)))
        };
        let italic = face(&mut t, Font::UiItalic).unwrap();
        assert_eq!(italic, (fontdb::Style::Italic, fontdb::Weight::NORMAL));
        let bold_italic = face(&mut t, Font::UiSemiboldItalic).unwrap();
        assert_eq!(
            bold_italic,
            (fontdb::Style::Italic, fontdb::Weight::SEMIBOLD)
        );
        assert_eq!(face(&mut t, Font::Ui).unwrap().0, fontdb::Style::Normal);
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

    #[test]
    fn wrapping_the_first_lines_measures_only_those() {
        // A long description in a phone's narrow board column (UR-55).
        let text = (0..2000)
            .map(|n| format!("word{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut all = TextSystem::bundled_only();
        let every = all.wrap(&text, Font::Ui, 13.0, 15.0);
        let measured_all = all.shaped_count();
        let mut first = TextSystem::bundled_only();
        let two = first.wrap_first(&text, Font::Ui, 13.0, 15.0, 3);
        assert_eq!(two, every[..3]);
        assert!(
            first.shaped_count() * 50 < measured_all,
            "{} vs {measured_all}",
            first.shaped_count()
        );
        // Asking for more lines than there are gives them all.
        assert_eq!(
            first.wrap_first("a\nb", Font::Ui, 13.0, 500.0, 5),
            first.wrap("a\nb", Font::Ui, 13.0, 500.0)
        );
        assert!(
            first
                .wrap_first("a\nb", Font::Ui, 13.0, 500.0, 0)
                .is_empty()
        );
    }

    #[test]
    fn wraps_at_line_breaks_then_between_words() {
        let mut t = TextSystem::bundled_only();
        let lines = |t: &mut TextSystem, s: &str, w: f32| -> Vec<String> {
            t.wrap(s, Font::Ui, 13.0, w)
                .into_iter()
                .map(|r| s[r].to_owned())
                .collect()
        };
        assert_eq!(lines(&mut t, "one\n\ntwo", 500.0), ["one", "", "two"]);
        assert_eq!(lines(&mut t, "", 500.0), [""]);
        let three = t.measure("aaa bbb", Font::Ui, 13.0) + 1.0;
        assert_eq!(
            lines(&mut t, "aaa bbb ccc ddd", three),
            ["aaa bbb ", "ccc ddd"],
            "the space stays on the line before"
        );
        let narrow = t.measure("wwww", Font::Ui, 13.0) + 0.5;
        assert_eq!(
            lines(&mut t, "wwwwwwwwww x", narrow),
            ["wwww", "wwww", "ww x"],
            "a long word breaks between characters"
        );
        // Ranges of one paragraph touch; a `\n` sits between paragraphs.
        let r = t.wrap("aaa bbb ccc\nd", Font::Ui, 13.0, three);
        assert_eq!((r[0].end, r[1].start, r[1].end, r[2].start), (8, 8, 11, 12));
        assert_eq!(lines(&mut t, "wide", 0.0), ["w", "i", "d", "e"]);
    }
}
