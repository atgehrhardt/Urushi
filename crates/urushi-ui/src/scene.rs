//! The renderer-agnostic display list.
//!
//! Widgets paint into a [`Scene`], and a backend (`urushi-render`) draws it. Primitives
//! are drawn in order. Clips nest. [`Scene::dump`] gives a stable text form for snapshot
//! tests, so UI is testable without a GPU.

use std::fmt::Write;

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

/// A filled (optionally rounded, optionally bordered) rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// Bounds.
    pub rect: Rect,
    /// Fill.
    pub color: Color,
    /// Corner radius.
    pub radius: f32,
    /// Border width (0 = none). Drawn inside the bounds.
    pub border_width: f32,
    /// Border colour.
    pub border_color: Color,
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

/// One drawing operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// A rectangle.
    Quad(Quad),
    /// A line of text.
    Text(Text),
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
            radius: 0.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        });
    }

    /// Adds a rectangle with full control.
    pub fn quad(&mut self, quad: Quad) {
        if quad.rect.width > 0.0
            && quad.rect.height > 0.0
            && (quad.color.a > 0.0 || quad.border_width > 0.0)
        {
            self.primitives.push(Primitive::Quad(quad));
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
                    let border = if q.border_width > 0.0 {
                        format!(" border {:.0} {}", q.border_width, q.border_color.to_hex())
                    } else {
                        String::new()
                    };
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
                Primitive::PushClip(c) => writeln!(out, "clip {}", r(c)),
                Primitive::PopClip => writeln!(out, "unclip"),
            };
        }
        out
    }
}
