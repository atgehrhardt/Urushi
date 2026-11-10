//! Urushi's in-house widget toolkit (ADR 0006).
//!
//! Screens follow the Elm architecture: application state → `view()` → a [`Node`] tree →
//! [`layout::build`] → a [`Frame`] with a renderer-agnostic [`Scene`] plus hit regions.
//! Input is mapped back to messages through the frame (`Frame::click`). Nothing here
//! touches the GPU or the OS, so UI behaviour is testable by asserting on scenes.

pub mod access;
pub mod color;
pub mod field;
pub mod geometry;
pub mod layout;
pub mod node;
pub mod scene;
pub mod text;
pub mod textfield;
pub mod theme;
pub mod widgets;

pub use color::Color;
pub use field::{FieldRegion, caret_at};
pub use geometry::{Insets, Point, Rect, Size};
pub use layout::{AccessNode, Frame, Hit, ScrollRegion, build};
pub use node::{Align, CursorIcon, Flow, Key, Length, Node, Role, Shadow, key};
pub use scene::{Backdrop, Font, Image, ImageData, Primitive, Quad, Scene, Text};
pub use text::{CellMetrics, TextSystem};
pub use textfield::TextField;
pub use theme::{Glow, TerminalPalette, Theme};
