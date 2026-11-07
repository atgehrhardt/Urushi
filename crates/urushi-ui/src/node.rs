//! The view tree: what screens return from `view()`.
//!
//! A [`Node`] is a box with a [`Style`], optional content (text, a text field, or custom
//! painting), children, and optional interaction (a message to send when clicked). The
//! tree is rebuilt every frame from application state (Elm architecture). Layout and
//! painting are in `layout.rs`.

use std::rc::Rc;

use crate::color::Color;
use crate::geometry::{Insets, Rect};
use crate::scene::{Font, Scene};
use crate::text::TextSystem;

/// A stable identity for interactive nodes: hover, focus, accessibility, scrolling.
pub type Key = u64;

/// Builds a [`Key`] from string parts, e.g. `key(&["thread", &id])`.
pub fn key(parts: &[&str]) -> Key {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// How big a node is along one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// Exactly this many logical pixels.
    Fixed(f32),
    /// A share of the parent's leftover space, by weight.
    Fill(f32),
    /// Just big enough for the content.
    Fit,
}

/// Direction children are laid out in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    /// Left to right.
    Row,
    /// Top to bottom.
    #[default]
    Column,
    /// Layered: every child fills the box, later children on top (overlays, dialogs).
    Stack,
}

/// Placement along an axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// At the start.
    #[default]
    Start,
    /// Centred.
    Center,
    /// At the end.
    End,
    /// Fill the cross axis (cross-axis alignment only).
    Stretch,
    /// Spread with equal gaps (main-axis only).
    SpaceBetween,
}

/// Mouse cursor to show over a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// Arrow.
    #[default]
    Default,
    /// Hand (clickable).
    Pointer,
    /// I-beam (text).
    Text,
    /// Horizontal resize (a vertical divider).
    ResizeColumn,
    /// Vertical resize (a horizontal divider).
    ResizeRow,
}

/// Accessibility role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Not exposed.
    #[default]
    None,
    /// A button.
    Button,
    /// Static text.
    Label,
    /// A text input.
    TextInput,
    /// A selectable list item (thread, tab).
    Item,
    /// A dialog.
    Dialog,
    /// A terminal.
    Terminal,
    /// A switch.
    Switch,
}

/// Box styling.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// Child direction.
    pub flow: Flow,
    /// Width.
    pub width: Length,
    /// Height.
    pub height: Length,
    /// Inner padding.
    pub padding: Insets,
    /// Space between children.
    pub gap: f32,
    /// Main-axis placement of children.
    pub justify: Align,
    /// Cross-axis placement of children.
    pub align: Align,
    /// Fill colour.
    pub background: Option<Color>,
    /// Fill colour while hovered (interactive nodes only).
    pub hover_background: Option<Color>,
    /// Border `(width, colour)`.
    pub border: Option<(f32, Color)>,
    /// Corner radius.
    pub radius: f32,
    /// Clip children to this box.
    pub clip: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            flow: Flow::Column,
            width: Length::Fit,
            height: Length::Fit,
            padding: Insets::default(),
            gap: 0.0,
            justify: Align::Start,
            align: Align::Stretch,
            background: None,
            hover_background: None,
            border: None,
            radius: 0.0,
            clip: false,
        }
    }
}

/// Custom painting for a node's rectangle (e.g. terminal panes).
pub type Painter = Rc<dyn Fn(Rect, &mut Scene, &mut TextSystem)>;

/// What a node draws inside its box.
#[derive(Clone, Default)]
pub enum Content {
    /// Nothing (a plain container).
    #[default]
    None,
    /// A single line of text, truncated with an ellipsis if needed.
    Label {
        /// The text.
        text: String,
        /// Font.
        font: Font,
        /// Size.
        size: f32,
        /// Colour.
        color: Color,
    },
    /// An editable line of text.
    Field {
        /// Current text.
        text: String,
        /// Caret byte offset when focused.
        caret: Option<usize>,
        /// Shown when empty.
        placeholder: String,
        /// Text colour.
        color: Color,
        /// Placeholder colour.
        placeholder_color: Color,
        /// Caret colour.
        caret_color: Color,
        /// Size.
        size: f32,
    },
    /// Custom painting.
    Paint(Painter),
}

impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Label { text, .. } => write!(f, "Label({text:?})"),
            Self::Field { text, .. } => write!(f, "Field({text:?})"),
            Self::Paint(_) => write!(f, "Paint"),
        }
    }
}

/// Scroll state for a scrollable column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    /// Identifies the scroll area in [`crate::Frame::scroll_regions`].
    pub key: Key,
    /// Current offset from the top.
    pub offset: f32,
}

/// A node in the view tree.
#[derive(Clone, Debug)]
pub struct Node<M> {
    /// Styling.
    pub style: Style,
    /// What it draws.
    pub content: Content,
    /// Children.
    pub children: Vec<Node<M>>,
    /// Message sent on click.
    pub on_click: Option<M>,
    /// Identity for hover, focus and accessibility.
    pub key: Option<Key>,
    /// Cursor while hovered.
    pub cursor: CursorIcon,
    /// Accessibility role.
    pub role: Role,
    /// Accessibility label (falls back to label text).
    pub label: Option<String>,
    /// Makes this a vertically scrolling container.
    pub scroll: Option<Scroll>,
}

impl<M> Node<M> {
    /// An empty box.
    pub fn new() -> Self {
        Self {
            style: Style::default(),
            content: Content::None,
            children: Vec::new(),
            on_click: None,
            key: None,
            cursor: CursorIcon::Default,
            role: Role::None,
            label: None,
            scroll: None,
        }
    }

    /// A box laying children out left to right.
    pub fn row() -> Self {
        let mut node = Self::new();
        node.style.flow = Flow::Row;
        node.style.align = Align::Center;
        node
    }

    /// A box laying children out top to bottom.
    pub fn column() -> Self {
        Self::new()
    }

    /// A box layering its children on top of each other.
    pub fn stack() -> Self {
        let mut node = Self::new();
        node.style.flow = Flow::Stack;
        node
    }

    /// A line of text.
    pub fn text(text: impl Into<String>, font: Font, size: f32, color: Color) -> Self {
        let mut node = Self::new();
        node.content = Content::Label {
            text: text.into(),
            font,
            size,
            color,
        };
        node
    }

    /// An invisible flexible spacer.
    pub fn spacer() -> Self {
        Self::new()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
    }

    /// Sets the width.
    #[must_use]
    pub fn width(mut self, width: Length) -> Self {
        self.style.width = width;
        self
    }

    /// Sets the height.
    #[must_use]
    pub fn height(mut self, height: Length) -> Self {
        self.style.height = height;
        self
    }

    /// Sets the padding.
    #[must_use]
    pub fn padding(mut self, padding: Insets) -> Self {
        self.style.padding = padding;
        self
    }

    /// Sets the gap between children.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = gap;
        self
    }

    /// Sets main-axis placement.
    #[must_use]
    pub fn justify(mut self, justify: Align) -> Self {
        self.style.justify = justify;
        self
    }

    /// Sets cross-axis placement.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.style.align = align;
        self
    }

    /// Sets the background.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.style.background = Some(color);
        self
    }

    /// Sets the hovered background.
    #[must_use]
    pub fn hover_background(mut self, color: Color) -> Self {
        self.style.hover_background = Some(color);
        self
    }

    /// Sets a border.
    #[must_use]
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.style.border = Some((width, color));
        self
    }

    /// Sets the corner radius.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.style.radius = radius;
        self
    }

    /// Clips children to the box.
    #[must_use]
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
        self
    }

    /// Adds a child.
    #[must_use]
    pub fn child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Adds children.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Self>) -> Self {
        self.children.extend(children);
        self
    }

    /// Sends `message` when clicked. Also shows a pointer cursor.
    #[must_use]
    pub fn on_click(mut self, message: M) -> Self {
        self.on_click = Some(message);
        if self.cursor == CursorIcon::Default {
            self.cursor = CursorIcon::Pointer;
        }
        self
    }

    /// Sets the identity key.
    #[must_use]
    pub fn key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Sets the hover cursor.
    #[must_use]
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = cursor;
        self
    }

    /// Sets the accessibility role and label.
    #[must_use]
    pub fn role(mut self, role: Role, label: impl Into<String>) -> Self {
        self.role = role;
        self.label = Some(label.into());
        self
    }

    /// Paints with `painter` inside the box.
    #[must_use]
    pub fn paint(mut self, painter: impl Fn(Rect, &mut Scene, &mut TextSystem) + 'static) -> Self {
        self.content = Content::Paint(Rc::new(painter));
        self
    }

    /// Makes the box a vertically scrolling, clipped column.
    #[must_use]
    pub fn scroll(mut self, key: Key, offset: f32) -> Self {
        self.scroll = Some(Scroll { key, offset });
        self.style.clip = true;
        self
    }
}

impl<M> Default for Node<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_and_distinct() {
        assert_eq!(key(&["a", "b"]), key(&["a", "b"]));
        assert_ne!(key(&["a", "b"]), key(&["ab"]));
        assert_ne!(key(&["thread", "1"]), key(&["tab", "1"]));
    }
}
