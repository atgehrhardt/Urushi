//! The renderer-agnostic display list.
//!
//! Widgets paint into a [`Scene`], and a backend (`urushi-render`) draws it. Primitives
//! are drawn in order. Clips nest. [`Scene::dump`] gives a stable text form for snapshot
//! tests, so UI is testable without a GPU.

use std::fmt::Write;

use std::sync::Arc;

use crate::color::Color;
use crate::geometry::{Point, Rect};

/// Which bundled font a run of text uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    /// Inter Regular.
    Ui,
    /// Inter Medium.
    UiMedium,
    /// Inter, semibold weight.
    UiSemibold,
    /// `JetBrains Mono`.
    Mono {
        /// Bold weight.
        bold: bool,
        /// Italic.
        italic: bool,
    },
}

impl Font {
    /// Plain monospace.
    pub const MONO: Self = Self::Mono {
        bold: false,
        italic: false,
    };

    fn name(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::UiMedium => "ui-medium",
            Self::UiSemibold => "ui-semibold",
            Self::Mono {
                bold: false,
                italic: false,
            } => "mono",
            Self::Mono {
                bold: true,
                italic: false,
            } => "mono-bold",
            Self::Mono {
                bold: false,
                italic: true,
            } => "mono-italic",
            Self::Mono {
                bold: true,
                italic: true,
            } => "mono-bold-italic",
        }
    }
}

/// A filled (optionally rounded, bordered, soft-edged or gradient) rectangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quad {
    /// Bounds.
    pub rect: Rect,
    /// Fill (the top colour when `gradient` is set).
    pub color: Color,
    /// Corner radius.
    pub radius: f32,
    /// Border width (0 = none). Drawn inside the bounds.
    pub border_width: f32,
    /// Border colour.
    pub border_color: Color,
    /// Width of a feathered edge, centred on the outline (0 = crisp). Soft quads are
    /// drop shadows and glows, which would otherwise need a blur pass.
    pub softness: f32,
    /// Bottom colour of a top-to-bottom gradient (`None`: a flat fill).
    pub gradient: Option<Color>,
}

/// "Frost whatever is already drawn under this rounded rectangle": the renderer blurs
/// what's below and draws the blurred copy inside the shape. Glass panels put one under
/// their translucent fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// Bounds.
    pub rect: Rect,
    /// Corner radius.
    pub radius: f32,
    /// How far the blur spreads, in logical pixels.
    pub blur: f32,
}

/// One line of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// Top-left of the line box.
    pub origin: Point,
    /// The text (single line).
    pub text: String,
    /// Font.
    pub font: Font,
    /// Size in logical pixels.
    pub size: f32,
    /// Colour.
    pub color: Color,
}

/// A bitmap, such as the app's logo: straight (not premultiplied) RGBA rows, top to
/// bottom. The renderer scales it to the size it is drawn at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    /// Identifies the bitmap, so the renderer can cache it; distinct images need
    /// distinct ids.
    pub id: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl ImageData {
    /// An image from raw RGBA rows. `None` if `rgba` isn't `width * height * 4` bytes.
    pub fn new(id: u64, width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
        (width > 0 && height > 0 && rgba.len() == expected).then_some(Self {
            id,
            width,
            height,
            rgba,
        })
    }
}

/// A bitmap stretched over a rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// Where it goes.
    pub rect: Rect,
    /// The pixels.
    pub data: Arc<ImageData>,
}

/// One drawing operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// A rectangle.
    Quad(Quad),
    /// A line of text.
    Text(Text),
    /// A bitmap.
    Image(Image),
    /// A blurred copy of what's below.
    Backdrop(Backdrop),
    /// Restrict following primitives to `Rect` (intersected with any outer clip).
    PushClip(Rect),
    /// Undo the most recent `PushClip`.
    PopClip,
}

/// An ordered list of primitives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// The primitives, in paint order.
    pub primitives: Vec<Primitive>,
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a plain filled rectangle.
    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.quad(Quad {
            rect,
            color,
            ..Quad::default()
        });
    }

    /// Adds a rectangle with full control.
    pub fn quad(&mut self, quad: Quad) {
        if quad.rect.width > 0.0
            && quad.rect.height > 0.0
            && (quad.color.a > 0.0
                || quad.border_width > 0.0
                || quad.gradient.is_some_and(|c| c.a > 0.0))
        {
            self.primitives.push(Primitive::Quad(quad));
        }
    }

    /// Frosts what's already drawn under `rect` (see [`Backdrop`]).
    pub fn backdrop(&mut self, rect: Rect, radius: f32, blur: f32) {
        if rect.width > 0.0 && rect.height > 0.0 && blur > 0.0 {
            self.primitives
                .push(Primitive::Backdrop(Backdrop { rect, radius, blur }));
        }
    }

    /// Adds a line of text.
    pub fn text(
        &mut self,
        origin: Point,
        text: impl Into<String>,
        font: Font,
        size: f32,
        color: Color,
    ) {
        let text = text.into();
        if !text.is_empty() {
            self.primitives.push(Primitive::Text(Text {
                origin,
                text,
                font,
                size,
                color,
            }));
        }
    }

    /// Adds a bitmap stretched over `rect`.
    pub fn image(&mut self, rect: Rect, data: Arc<ImageData>) {
        if rect.width > 0.0 && rect.height > 0.0 {
            self.primitives.push(Primitive::Image(Image { rect, data }));
        }
    }

    /// Starts clipping to `rect`.
    pub fn push_clip(&mut self, rect: Rect) {
        self.primitives.push(Primitive::PushClip(rect));
    }

    /// Ends the most recent clip.
    pub fn pop_clip(&mut self) {
        self.primitives.push(Primitive::PopClip);
    }

    /// A stable, line-per-primitive text form for snapshot tests.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let r = |r: &Rect| format!("{:.0},{:.0} {:.0}x{:.0}", r.x, r.y, r.width, r.height);
        for p in &self.primitives {
            let _ = match p {
                Primitive::Quad(q) => {
                    let mut border = if q.border_width > 0.0 {
                        format!(" border {:.0} {}", q.border_width, q.border_color.to_hex())
                    } else {
                        String::new()
                    };
                    if let Some(bottom) = q.gradient {
                        let _ = write!(border, " to {}", bottom.to_hex());
                    }
                    if q.softness > 0.0 {
                        let _ = write!(border, " soft {:.0}", q.softness);
                    }
                    writeln!(
                        out,
                        "quad {} {} r{:.0}{border}",
                        r(&q.rect),
                        q.color.to_hex(),
                        q.radius
                    )
                }
                Primitive::Text(t) => writeln!(
                    out,
                    "text {:.0},{:.0} {:?} {} {:.0} {}",
                    t.origin.x,
                    t.origin.y,
                    t.text,
                    t.font.name(),
                    t.size,
                    t.color.to_hex()
                ),
                Primitive::Image(i) => writeln!(out, "image {} #{}", r(&i.rect), i.data.id),
                Primitive::Backdrop(b) => writeln!(
                    out,
                    "backdrop {} r{:.0} blur {:.0}",
                    r(&b.rect),
                    b.radius,
                    b.blur
                ),
                Primitive::PushClip(c) => writeln!(out, "clip {}", r(c)),
                Primitive::PopClip => writeln!(out, "unclip"),
            };
        }
        out
    }
}
