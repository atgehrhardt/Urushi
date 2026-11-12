//! Laying out a view tree, painting it into a [`Scene`], and recording where things are
//! (for clicks, hover, cursors, scrolling and accessibility).
//!
//! Layout is a simplified flexbox, in two passes:
//! 1. **Measure** (bottom-up): the natural size of `Fit` nodes.
//! 2. **Place** (top-down): `Fixed` children get their size, `Fit` children their
//!    measured size, and `Fill` children share the rest by weight.

use crate::color::Color;
use crate::field::{self, FieldRegion};
use crate::geometry::{Point, Rect, Size};
use crate::node::{Align, Axis, Content, CursorIcon, Flow, Key, Length, Node, Role, Style};
use crate::scene::{Quad, Scene};
use crate::text::TextSystem;

/// Width a text field asks for when it is sized to fit.
const FIELD_FIT_WIDTH: f32 = 160.0;

/// An interactive region.
#[derive(Clone, Debug)]
pub struct Hit<M> {
    /// Where it is (already clipped).
    pub rect: Rect,
    /// Its key, if any.
    pub key: Option<Key>,
    /// Message on click.
    pub on_click: Option<M>,
    /// Message on secondary click.
    pub on_secondary_click: Option<M>,
    /// Message when a drag starts here.
    pub on_drag: Option<M>,
    /// Message when a drag is released here.
    pub on_drop: Option<M>,
    /// Cursor while hovered.
    pub cursor: CursorIcon,
    /// Clicks here don't reach regions under it (see [`Node::opaque`]).
    pub opaque: bool,
}

/// A scrollable area and how far its content extends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRegion {
    /// Key from the node's `scroll`.
    pub key: Key,
    /// Visible area.
    pub rect: Rect,
    /// Which way it scrolls.
    pub axis: Axis,
    /// Length of the content along that axis (its height, or sideways its width).
    pub content_len: f32,
    /// Offset actually used (clamped).
    pub offset: f32,
}

impl ScrollRegion {
    /// The largest valid offset.
    pub fn max_offset(&self) -> f32 {
        let shown = match self.axis {
            Axis::Vertical => self.rect.height,
            Axis::Horizontal => self.rect.width,
        };
        (self.content_len - shown).max(0.0)
    }
}

/// An accessible element.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessNode {
    /// Stable id (the node key, or a position-derived fallback).
    pub id: u64,
    /// Role.
    pub role: Role,
    /// Spoken label.
    pub label: String,
    /// Bounds.
    pub rect: Rect,
    /// Can be clicked.
    pub clickable: bool,
}

/// The result of laying out and painting a tree.
#[derive(Clone, Debug)]
pub struct Frame<M> {
    /// What to draw.
    pub scene: Scene,
    /// Interactive regions, in paint order (last is topmost).
    pub hits: Vec<Hit<M>>,
    /// Scrollable areas, in paint order.
    pub scroll_regions: Vec<ScrollRegion>,
    /// Accessible elements, in paint order.
    pub access: Vec<AccessNode>,
    /// Text fields, in paint order.
    pub fields: Vec<FieldRegion>,
}

impl<M: Clone> Frame<M> {
    /// The topmost interactive region at `p`.
    pub fn hit(&self, p: Point) -> Option<&Hit<M>> {
        self.hits.iter().rev().find(|h| h.rect.contains(p))
    }

    /// The message to send for a click at `p`: the topmost region there that takes
    /// clicks, unless an opaque region lies above it.
    pub fn click(&self, p: Point) -> Option<M> {
        self.topmost(p, |h| h.on_click.as_ref())
    }

    /// The message for a secondary click (right / two-finger click) at `p`.
    pub fn secondary_click(&self, p: Point) -> Option<M> {
        self.topmost(p, |h| h.on_secondary_click.as_ref())
    }

    /// The message to send if a drag starts at `p`.
    pub fn drag_source(&self, p: Point) -> Option<M> {
        self.topmost(p, |h| h.on_drag.as_ref())
    }

    /// The message to send if a drag is released at `p`.
    pub fn drop_target(&self, p: Point) -> Option<M> {
        self.topmost(p, |h| h.on_drop.as_ref())
    }

    /// What `pick` finds in the topmost region at `p` that has one, looking no further
    /// down than the first opaque region.
    fn topmost(&self, p: Point, pick: impl Fn(&Hit<M>) -> Option<&M>) -> Option<M> {
        for h in self.hits.iter().rev().filter(|h| h.rect.contains(p)) {
            if let Some(m) = pick(h) {
                return Some(m.clone());
            }
            if h.opaque {
                return None;
            }
        }
        None
    }

    /// Whether the pointer moving from `from` to `to` changes what this frame draws as
    /// hovered: if not, there is nothing to draw again (ADR 0050).
    pub fn hover_changes(&self, from: Option<Point>, to: Option<Point>) -> bool {
        let inside = |p: Option<Point>, r: &Rect| p.is_some_and(|p| r.contains(p));
        self.hits
            .iter()
            .filter(|h| h.on_click.is_some() || h.on_drop.is_some())
            .any(|h| inside(from, &h.rect) != inside(to, &h.rect))
    }

    /// The cursor to show at `p`.
    pub fn cursor(&self, p: Point) -> CursorIcon {
        self.hit(p).map_or(CursorIcon::Default, |h| h.cursor)
    }

    /// The topmost text field at `p`, if it's visible there.
    pub fn field_at(&self, p: Point) -> Option<&FieldRegion> {
        let visible = |f: &&FieldRegion| {
            f.key.is_none_or(|k| {
                self.hits
                    .iter()
                    .rev()
                    .find(|h| h.rect.contains(p))
                    .is_some_and(|h| h.key == Some(k))
            })
        };
        self.fields
            .iter()
            .rev()
            .filter(|f| f.rect.contains(p))
            .find(visible)
    }

    /// The text field whose widget has `key` (e.g. to keep selecting by drag after the
    /// pointer has left it).
    pub fn field(&self, key: Key) -> Option<&FieldRegion> {
        self.fields.iter().rev().find(|f| f.key == Some(key))
    }

    /// The innermost scroll area at `p`.
    pub fn scroll_region(&self, p: Point) -> Option<&ScrollRegion> {
        self.scroll_regions
            .iter()
            .rev()
            .find(|r| r.rect.contains(p))
    }

    /// The innermost area at `p` that scrolls along `axis`: a wheel or finger moving
    /// sideways scrolls the row a column sits in, not the column.
    pub fn scroll_region_along(&self, p: Point, axis: Axis) -> Option<&ScrollRegion> {
        self.scroll_regions
            .iter()
            .rev()
            .find(|r| r.axis == axis && r.rect.contains(p))
    }
}

/// Lays out `root` inside `bounds` and paints it. `pointer` is the mouse position, used
/// for hover styling.
pub fn build<M: Clone>(
    root: &Node<M>,
    bounds: Rect,
    text: &mut TextSystem,
    pointer: Option<Point>,
) -> Frame<M> {
    let mut ctx = Ctx {
        text,
        pointer,
        frame: Frame {
            scene: Scene::new(),
            hits: Vec::new(),
            scroll_regions: Vec::new(),
            access: Vec::new(),
            fields: Vec::new(),
        },
        clips: vec![bounds],
        owner: None,
    };
    ctx.place(root, bounds, &mut 0);
    ctx.frame
}

struct Ctx<'t, M> {
    text: &'t mut TextSystem,
    pointer: Option<Point>,
    frame: Frame<M>,
    clips: Vec<Rect>,
    /// Key of the innermost keyed node being placed (a field's widget).
    owner: Option<Key>,
}

fn main_axis(flow: Flow, size: Size) -> f32 {
    match flow {
        Flow::Row => size.width,
        Flow::Column | Flow::Stack => size.height,
    }
}

fn cross_axis(flow: Flow, size: Size) -> f32 {
    match flow {
        Flow::Row => size.height,
        Flow::Column | Flow::Stack => size.width,
    }
}

impl<M: Clone> Ctx<'_, M> {
    /// Natural size of a node (what `Fit` means). `Fill` counts as zero here.
    fn measure(&mut self, node: &Node<M>) -> Size {
        let content = match &node.content {
            Content::Label {
                text, font, size, ..
            } => Size::new(
                self.text.measure(text, *font, *size).ceil(),
                TextSystem::line_height(*size),
            ),
            Content::Field { size, .. } => {
                Size::new(FIELD_FIT_WIDTH, TextSystem::line_height(*size))
            }
            Content::None | Content::Image(_) | Content::Paint(_) => Size::default(),
        };
        let s = &node.style;
        let mut main = 0.0f32;
        let mut cross = 0.0f32;
        for (i, child) in node.children.iter().enumerate() {
            let size = self.resolved_size(child);
            if s.flow == Flow::Stack {
                main = main.max(main_axis(s.flow, size));
            } else {
                main += main_axis(s.flow, size) + if i > 0 { s.gap } else { 0.0 };
            }
            cross = cross.max(cross_axis(s.flow, size));
        }
        let children = match s.flow {
            Flow::Row => Size::new(main, cross),
            Flow::Column | Flow::Stack => Size::new(cross, main),
        };
        Size::new(
            content.width.max(children.width) + s.padding.horizontal(),
            content.height.max(children.height) + s.padding.vertical(),
        )
    }

    /// Size along both axes, with `Fixed` applied and `Fill` as zero.
    fn resolved_size(&mut self, node: &Node<M>) -> Size {
        let natural = if matches!(node.style.width, Length::Fit)
            || matches!(node.style.height, Length::Fit)
        {
            self.measure(node)
        } else {
            Size::default()
        };
        let natural = Size::new(
            natural.width,
            node.style
                .max_height
                .map_or(natural.height, |max| natural.height.min(max)),
        );
        let pick = |len: Length, natural: f32| match len {
            Length::Fixed(v) => v,
            Length::Fit => natural,
            Length::Fill(_) => 0.0,
        };
        Size::new(
            pick(node.style.width, natural.width),
            pick(node.style.height, natural.height),
        )
    }

    fn place(&mut self, node: &Node<M>, rect: Rect, index: &mut u64) {
        *index += 1;
        let my_index = *index;
        let s = &node.style;
        let rect = if s.slide == 0.0 && s.slide_y == 0.0 {
            rect
        } else {
            Rect::new(
                rect.x - rect.width * s.slide,
                rect.y - rect.height * s.slide_y,
                rect.width,
                rect.height,
            )
        };
        let clip = *self.clips.last().unwrap_or(&rect);
        let visible = rect.intersect(&clip);
        let hovered = (node.on_click.is_some() || node.on_drop.is_some())
            && self.pointer.is_some_and(|p| visible.contains(p));

        self.paint_box(s, rect, clip, hovered);

        // Interaction and accessibility.
        if node.on_click.is_some()
            || node.on_secondary_click.is_some()
            || node.on_drag.is_some()
            || node.on_drop.is_some()
            || node.cursor != CursorIcon::Default
            || node.key.is_some()
            || node.opaque
        {
            self.frame.hits.push(Hit {
                rect: visible,
                key: node.key,
                on_click: node.on_click.clone(),
                on_secondary_click: node.on_secondary_click.clone(),
                on_drag: node.on_drag.clone(),
                on_drop: node.on_drop.clone(),
                cursor: node.cursor,
                opaque: node.opaque,
            });
        }
        if node.role != Role::None {
            let label = node
                .label
                .clone()
                .or_else(|| label_text(node))
                .unwrap_or_default();
            self.frame.access.push(AccessNode {
                id: node.key.unwrap_or(my_index),
                role: node.role,
                label,
                rect: visible,
                clickable: node.on_click.is_some(),
            });
        }

        let inner = rect.inset(s.padding);
        self.paint_content(node, inner);

        if node.children.is_empty() {
            return;
        }
        let outer_owner = self.owner;
        self.owner = node.key.or(outer_owner);
        if s.clip {
            let c = inner.intersect(&clip);
            self.frame.scene.push_clip(c);
            self.clips.push(c);
        }
        self.place_children(node, inner, index);
        if s.clip {
            self.clips.pop();
            self.frame.scene.pop_clip();
        }
        self.owner = outer_owner;
    }

    /// A node's own box: its shadow, frosted backdrop, background (or hover fill) and
    /// border, in that order.
    fn paint_box(&mut self, s: &Style, rect: Rect, clip: Rect, hovered: bool) {
        if let Some(shadow) = s.shadow {
            self.frame.scene.quad(Quad {
                rect: Rect::new(rect.x, rect.y + shadow.offset, rect.width, rect.height),
                color: shadow.color,
                radius: s.radius + shadow.blur / 2.0,
                softness: shadow.blur,
                ..Quad::default()
            });
        }
        if s.backdrop_blur > 0.0 {
            self.frame
                .scene
                .backdrop(rect.intersect(&clip), s.radius, s.backdrop_blur);
        }
        let fill = if hovered {
            s.hover_background.or(s.background)
        } else {
            s.background
        };
        if fill.is_some() || s.border.is_some() {
            let (border_width, border_color) = s.border.unwrap_or((0.0, Color::TRANSPARENT));
            self.frame.scene.quad(Quad {
                rect,
                color: fill.unwrap_or(Color::TRANSPARENT),
                radius: s.radius,
                border_width,
                border_color,
                // A hover fill replaces the gradient; the resting fill keeps it.
                gradient: if hovered && s.hover_background.is_some() {
                    None
                } else {
                    s.gradient
                },
                softness: 0.0,
            });
        }
    }

    fn paint_content(&mut self, node: &Node<M>, inner: Rect) {
        match &node.content {
            Content::None => {}
            Content::Label {
                text,
                font,
                size,
                color,
            } => {
                let shown = self
                    .text
                    .truncate(text, *font, *size, inner.width.max(0.0) + 0.5);
                let y = inner.y + (inner.height - TextSystem::line_height(*size)) / 2.0;
                self.frame
                    .scene
                    .text(Point::new(inner.x, y.round()), shown, *font, *size, *color);
            }
            Content::Field {
                text,
                caret,
                selection,
                placeholder,
                color,
                placeholder_color,
                caret_color,
                selection_color,
                size,
                multiline,
            } => {
                let look = field::Look {
                    text,
                    caret: *caret,
                    selection: *selection,
                    placeholder,
                    colors: [*color, *placeholder_color, *caret_color, *selection_color],
                    size: *size,
                    multiline: *multiline,
                };
                let region =
                    field::paint(&mut self.frame.scene, self.text, inner, &look, self.owner);
                self.frame.fields.push(region);
            }
            Content::Image(data) => self.frame.scene.image(inner, data.clone()),
            Content::Paint(painter) => painter(inner, &mut self.frame.scene, self.text),
        }
    }

    fn place_children(&mut self, node: &Node<M>, inner: Rect, index: &mut u64) {
        let s = &node.style;
        if s.flow == Flow::Stack && node.scroll.is_none() {
            for child in &node.children {
                let rect = match child.position {
                    Some(at) => positioned(at, self.resolved_size(child), inner),
                    None => inner,
                };
                self.place(child, rect, index);
            }
            return;
        }
        let flow = match node.scroll.map(|sc| sc.axis) {
            Some(Axis::Vertical) => Flow::Column,
            Some(Axis::Horizontal) => Flow::Row,
            None => s.flow,
        };
        let sizes: Vec<Size> = node
            .children
            .iter()
            .map(|c| self.resolved_size(c))
            .collect();
        let gaps = s.gap * count(node.children.len().saturating_sub(1));
        let inner_main = main_axis(flow, Size::new(inner.width, inner.height));
        let inner_cross = cross_axis(flow, Size::new(inner.width, inner.height));
        let weights: f32 = node.children.iter().map(|c| fill_weight(flow, c)).sum();
        let used: f32 = sizes.iter().map(|sz| main_axis(flow, *sz)).sum::<f32>() + gaps;
        // A scrolling column's content keeps its own height; a sideways row's `Fill`
        // children share what is there (and scroll past it only for their minimums).
        let leftover = if node.scroll.is_some_and(|sc| sc.axis == Axis::Vertical) {
            0.0
        } else {
            (inner_main - used).max(0.0)
        };

        let mains: Vec<f32> = node
            .children
            .iter()
            .zip(&sizes)
            .map(|(c, sz)| {
                let w = fill_weight(flow, c);
                if w > 0.0 && weights > 0.0 {
                    let share = leftover * w / weights;
                    match (flow, c.style.min_width) {
                        (Flow::Row, Some(min)) => share.max(min),
                        _ => share,
                    }
                } else {
                    main_axis(flow, *sz)
                }
            })
            .collect();
        let total: f32 = mains.iter().sum::<f32>() + gaps;

        let mut offset = 0.0;
        let mut extra_gap = 0.0;
        if weights == 0.0 {
            let free = (inner_main - total).max(0.0);
            match s.justify {
                Align::Center => offset = free / 2.0,
                Align::End => offset = free,
                Align::SpaceBetween if node.children.len() > 1 => {
                    extra_gap = free / count(node.children.len() - 1);
                }
                _ => {}
            }
        }
        if let Some(scroll) = node.scroll {
            let region = ScrollRegion {
                key: scroll.key,
                rect: inner,
                axis: scroll.axis,
                content_len: total,
                offset: 0.0,
            };
            let clamped = scroll.offset.clamp(0.0, region.max_offset());
            offset -= clamped;
            self.frame.scroll_regions.push(ScrollRegion {
                offset: clamped,
                ..region
            });
        }

        for ((child, size), main) in node.children.iter().zip(&sizes).zip(&mains) {
            let cross_len = match (flow, child.style.width, child.style.height) {
                (Flow::Column | Flow::Stack, Length::Fill(_), _)
                | (Flow::Row, _, Length::Fill(_)) => inner_cross,
                _ if s.align == Align::Stretch && cross_is_fit(flow, child) => inner_cross,
                _ => cross_axis(flow, *size).min(inner_cross),
            };
            let cross_offset = match s.align {
                Align::Center => (inner_cross - cross_len) / 2.0,
                Align::End => inner_cross - cross_len,
                _ => 0.0,
            };
            let rect = match flow {
                Flow::Row => Rect::new(inner.x + offset, inner.y + cross_offset, *main, cross_len),
                Flow::Column | Flow::Stack => {
                    Rect::new(inner.x + cross_offset, inner.y + offset, cross_len, *main)
                }
            };
            self.place_child(child, rect, index, node.scroll.is_some());
            offset += main + s.gap + extra_gap;
        }
    }
}

impl<M: Clone> Ctx<'_, M> {
    /// Places one child at `rect`. In a scrolling box, one outside the clip is skipped,
    /// so a long list costs what it shows (UR-66, ADR 0050); its ids are still counted,
    /// so those of the nodes after it don't change as it scrolls.
    fn place_child(&mut self, child: &Node<M>, rect: Rect, index: &mut u64, scrolling: bool) {
        let in_sight = self.clips.last().is_none_or(|clip| overlaps(rect, *clip));
        if scrolling && !in_sight {
            *index += subtree_len(child);
        } else {
            self.place(child, rect, index);
        }
    }
}

/// Whether `a` and `b` share any area.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// How many nodes `node` and everything under it are.
fn subtree_len<M>(node: &Node<M>) -> u64 {
    1 + node.children.iter().map(subtree_len).sum::<u64>()
}

/// A rectangle of `size` at `at`, moved (not shrunk, unless it can't fit) to stay inside
/// `bounds`, so a menu opened near an edge stays on screen.
fn positioned(at: Point, size: Size, bounds: Rect) -> Rect {
    let width = size.width.min(bounds.width);
    let height = size.height.min(bounds.height);
    let x = at.x.min(bounds.x + bounds.width - width).max(bounds.x);
    let y = at.y.min(bounds.y + bounds.height - height).max(bounds.y);
    Rect::new(x, y, width, height)
}

/// Child counts are tiny, so the conversion is exact.
#[allow(clippy::cast_precision_loss, reason = "small counts")]
fn count(n: usize) -> f32 {
    n as f32
}

fn fill_weight<M>(flow: Flow, node: &Node<M>) -> f32 {
    match (flow, node.style.width, node.style.height) {
        (Flow::Row, Length::Fill(w), _) | (Flow::Column | Flow::Stack, _, Length::Fill(w)) => w,
        _ => 0.0,
    }
}

fn cross_is_fit<M>(flow: Flow, node: &Node<M>) -> bool {
    match flow {
        Flow::Row => node.style.height == Length::Fit,
        Flow::Column | Flow::Stack => node.style.width == Length::Fit,
    }
}

fn label_text<M>(node: &Node<M>) -> Option<String> {
    match &node.content {
        Content::Label { text, .. } | Content::Field { text, .. } => Some(text.clone()),
        _ => node.children.iter().find_map(label_text),
    }
}
