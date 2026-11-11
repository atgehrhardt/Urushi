//! The view tree: what screens return from `view()`.
//!
//! A [`Node`] is a box with a [`Style`], optional content (text, a text field, or custom
//! painting), children, and optional interaction (a message to send when clicked). The
//! tree is rebuilt every frame from application state (Elm architecture). Layout and
//! painting are in `layout.rs`.

use std::rc::Rc;

use crate::color::Color;
use crate::geometry::{Insets, Point, Rect};
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
    /// Shifts the box (and everything in it) left by this fraction of its own width:
    /// 0 is in place, 1 is fully to the left. For panels that slide in.
    pub slide: f32,
    /// Shifts the box (and everything in it) up by this fraction of its own height:
    /// 0 is in place, 1 is fully above. For panels that drop down.
    pub slide_y: f32,
    /// Bottom colour of a top-to-bottom gradient over the background.
    pub gradient: Option<Color>,
    /// A soft shadow drawn under the box.
    pub shadow: Option<Shadow>,
    /// Frosts what's under the box with a blur of this many logical pixels (0: none).
    pub backdrop_blur: f32,
    /// The most a `Fit` height may grow to; past it the box keeps this height (make it
    /// scroll to reach the rest).
    pub max_height: Option<f32>,
    /// The least a `Fill` width gets in a row: when the row's share is smaller, the box
    /// keeps this width and the row runs past its end (make it scroll sideways).
    pub min_width: Option<f32>,
}

/// A soft drop shadow under a box: the box's shape, moved down by `offset` and
/// feathered over `blur` logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Colour at its darkest.
    pub color: Color,
    /// Width of the feathered edge.
    pub blur: f32,
    /// How far below the box it falls.
    pub offset: f32,
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
            slide: 0.0,
            slide_y: 0.0,
            gradient: None,
            shadow: None,
            backdrop_blur: 0.0,
            max_height: None,
            min_width: None,
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
    /// Editable text: one line, or several wrapped to the box (`multiline`).
    Field {
        /// Current text.
        text: String,
        /// Caret byte offset when focused.
        caret: Option<usize>,
        /// Selected byte range when focused.
        selection: Option<(usize, usize)>,
        /// Shown when empty.
        placeholder: String,
        /// Text colour.
        color: Color,
        /// Placeholder colour.
        placeholder_color: Color,
        /// Caret colour.
        caret_color: Color,
        /// Selection highlight colour.
        selection_color: Color,
        /// Size.
        size: f32,
        /// Wraps and keeps line breaks, rather than scrolling one line sideways.
        multiline: bool,
    },
    /// A bitmap, stretched over the box.
    Image(std::sync::Arc<crate::scene::ImageData>),
    /// Custom painting.
    Paint(Painter),
}

impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Label { text, .. } => write!(f, "Label({text:?})"),
            Self::Field { text, .. } => write!(f, "Field({text:?})"),
            Self::Image(i) => write!(f, "Image(#{})", i.id),
            Self::Paint(_) => write!(f, "Paint"),
        }
    }
}

/// Which way a scrolling box scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    /// Up and down: a column.
    #[default]
    Vertical,
    /// Sideways: a row.
    Horizontal,
}

/// Scroll state for a scrollable column or row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    /// Identifies the scroll area in [`crate::Frame::scroll_regions`].
    pub key: Key,
    /// Current offset from the top (or, sideways, from the left).
    pub offset: f32,
    /// Which way it scrolls.
    pub axis: Axis,
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
    /// Message sent on a secondary click (right click, or two-finger click on a trackpad).
    pub on_secondary_click: Option<M>,
    /// Message sent when the pointer is dragged starting on this node.
    pub on_drag: Option<M>,
    /// Message sent when a drag ends over this node.
    pub on_drop: Option<M>,
    /// Inside a [`Flow::Stack`], places this node at this point (its natural size,
    /// clamped to the stack), instead of filling the stack. For menus and popups.
    pub position: Option<Point>,
    /// Clicks on this node's own space (not on a child that takes them) stop here
    /// instead of reaching what is under it, e.g. a dialog over a backdrop that closes
    /// it.
    pub opaque: bool,
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
            on_secondary_click: None,
            on_drag: None,
            on_drop: None,
            position: None,
            opaque: false,
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

    /// A bitmap of `width` × `height` logical pixels.
    pub fn image(data: std::sync::Arc<crate::scene::ImageData>, width: f32, height: f32) -> Self {
        let mut node = Self::new();
        node.content = Content::Image(data);
        node.width(Length::Fixed(width))
            .height(Length::Fixed(height))
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

    /// Fades the background from its colour at the top to `bottom`.
    #[must_use]
    pub fn gradient(mut self, bottom: Color) -> Self {
        self.style.gradient = Some(bottom);
        self
    }

    /// Casts a soft shadow under the box.
    #[must_use]
    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.style.shadow = Some(shadow);
        self
    }

    /// Frosts what's drawn under the box (glass): a blur of `blur` logical pixels,
    /// under the background. Costs extra GPU passes, so keep it to overlays.
    #[must_use]
    pub fn backdrop_blur(mut self, blur: f32) -> Self {
        self.style.backdrop_blur = blur;
        self
    }

    /// Clips children to the box.
    #[must_use]
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
        self
    }

    /// Caps a `Fit` height at `height`.
    #[must_use]
    pub fn max_height(mut self, height: f32) -> Self {
        self.style.max_height = Some(height);
        self
    }

    /// Keeps a `Fill` width in a row at least `width` wide (see [`Style::min_width`]).
    #[must_use]
    pub fn min_width(mut self, width: f32) -> Self {
        self.style.min_width = Some(width);
        self
    }

    /// Stops clicks on this node from reaching what is under it (see [`Node::opaque`]).
    #[must_use]
    pub fn opaque(mut self) -> Self {
        self.opaque = true;
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

    /// Sends `message` on a secondary click (right click / two-finger click), e.g. to
    /// open a context menu.
    #[must_use]
    pub fn on_secondary_click(mut self, message: M) -> Self {
        self.on_secondary_click = Some(message);
        self
    }

    /// Sends `message` when a drag starts on this node. The runner decides when a press
    /// has moved far enough to count as a drag.
    #[must_use]
    pub fn on_drag(mut self, message: M) -> Self {
        self.on_drag = Some(message);
        self
    }

    /// Sends `message` when a drag is released over this node. While set, the node also
    /// shows its hover background under the pointer, as a drop highlight.
    #[must_use]
    pub fn on_drop(mut self, message: M) -> Self {
        self.on_drop = Some(message);
        self
    }

    /// Shifts this node left by `fraction` of its width (see [`Style::slide`]). Put it
    /// in a clipped parent so the part outside doesn't show.
    #[must_use]
    pub fn slide_x(mut self, fraction: f32) -> Self {
        self.style.slide = fraction;
        self
    }

    /// Shifts this node up by `fraction` of its height (see [`Style::slide_y`]). Put it
    /// in a clipped parent so the part outside doesn't show.
    #[must_use]
    pub fn slide_y(mut self, fraction: f32) -> Self {
        self.style.slide_y = fraction;
        self
    }

    /// Places this node at `at` inside its [`Flow::Stack`] parent, at its natural size.
    #[must_use]
    pub fn at(mut self, at: Point) -> Self {
        self.position = Some(at);
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
        self.scroll = Some(Scroll {
            key,
            offset,
            axis: Axis::Vertical,
        });
        self.style.clip = true;
        self
    }

    /// Makes the box a sideways scrolling, clipped row. Unlike a scrolling column, its
    /// `Fill` children still share the width that is there; it scrolls when their
    /// [`Node::min_width`]s need more.
    #[must_use]
    pub fn scroll_x(mut self, key: Key, offset: f32) -> Self {
        self.scroll = Some(Scroll {
            key,
            offset,
            axis: Axis::Horizontal,
        });
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
