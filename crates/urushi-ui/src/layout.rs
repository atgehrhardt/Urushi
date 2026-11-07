//! Laying out a view tree, painting it into a [`Scene`], and recording where things are
//! (for clicks, hover, cursors, scrolling and accessibility).
//!
//! Layout is a simplified flexbox, in two passes:
//! 1. **Measure** (bottom-up): the natural size of `Fit` nodes.
//! 2. **Place** (top-down): `Fixed` children get their size, `Fit` children their
//!    measured size, and `Fill` children share the rest by weight.

use crate::color::Color;
use crate::geometry::{Point, Rect, Size};
use crate::node::{Align, Content, CursorIcon, Flow, Key, Length, Node, Role};
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
    /// Cursor while hovered.
    pub cursor: CursorIcon,
}

/// A scrollable area and how far its content extends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRegion {
    /// Key from the node's `scroll`.
    pub key: Key,
    /// Visible area.
    pub rect: Rect,
    /// Total height of the content.
    pub content_height: f32,
    /// Offset actually used (clamped).
    pub offset: f32,
}

impl ScrollRegion {
    /// The largest valid offset.
    pub fn max_offset(&self) -> f32 {
        (self.content_height - self.rect.height).max(0.0)
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
}

impl<M: Clone> Frame<M> {
    /// The topmost interactive region at `p`.
    pub fn hit(&self, p: Point) -> Option<&Hit<M>> {
        self.hits.iter().rev().find(|h| h.rect.contains(p))
    }

    /// The message to send for a click at `p`.
    pub fn click(&self, p: Point) -> Option<M> {
        self.hits
            .iter()
            .rev()
            .filter(|h| h.rect.contains(p))
            .find_map(|h| h.on_click.clone())
    }

    /// The cursor to show at `p`.
    pub fn cursor(&self, p: Point) -> CursorIcon {
        self.hit(p).map_or(CursorIcon::Default, |h| h.cursor)
    }

    /// The innermost scroll area at `p`.
    pub fn scroll_region(&self, p: Point) -> Option<&ScrollRegion> {
        self.scroll_regions
            .iter()
            .rev()
            .find(|r| r.rect.contains(p))
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
        },
        clips: vec![bounds],
    };
    ctx.place(root, bounds, &mut 0);
    ctx.frame
}

struct Ctx<'t, M> {
    text: &'t mut TextSystem,
    pointer: Option<Point>,
    frame: Frame<M>,
    clips: Vec<Rect>,
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
            Content::None | Content::Paint(_) => Size::default(),
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
        let clip = *self.clips.last().unwrap_or(&rect);
        let visible = rect.intersect(&clip);
        let hovered = node.on_click.is_some() && self.pointer.is_some_and(|p| visible.contains(p));

        // Background and border.
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
            });
        }

        // Interaction and accessibility.
        if node.on_click.is_some() || node.cursor != CursorIcon::Default || node.key.is_some() {
            self.frame.hits.push(Hit {
                rect: visible,
                key: node.key,
                on_click: node.on_click.clone(),
                cursor: node.cursor,
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
                placeholder,
                color,
                placeholder_color,
                caret_color,
                size,
            } => {
                let line = TextSystem::line_height(*size);
                let y = (inner.y + (inner.height - line) / 2.0).round();
                self.frame.scene.push_clip(inner);
                if text.is_empty() {
                    self.frame.scene.text(
                        Point::new(inner.x, y),
                        placeholder.clone(),
                        crate::scene::Font::Ui,
                        *size,
                        *placeholder_color,
                    );
                } else {
                    self.frame.scene.text(
                        Point::new(inner.x, y),
                        text.clone(),
                        crate::scene::Font::Ui,
                        *size,
                        *color,
                    );
                }
                if let Some(caret) = caret {
                    let before = text.get(..*caret).unwrap_or(text);
                    let x = inner.x + self.text.measure(before, crate::scene::Font::Ui, *size);
                    self.frame
                        .scene
                        .rect(Rect::new(x.round(), y + 2.0, 1.5, line - 4.0), *caret_color);
                }
                self.frame.scene.pop_clip();
            }
            Content::Paint(painter) => painter(inner, &mut self.frame.scene, self.text),
        }
    }

    fn place_children(&mut self, node: &Node<M>, inner: Rect, index: &mut u64) {
        let s = &node.style;
        if s.flow == Flow::Stack && node.scroll.is_none() {
            for child in &node.children {
                self.place(child, inner, index);
            }
            return;
        }
        let flow = if node.scroll.is_some() {
            Flow::Column
        } else {
            s.flow
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
        let leftover = if node.scroll.is_some() {
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
                    leftover * w / weights
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
                content_height: total,
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
            self.place(child, rect, index);
            offset += main + s.gap + extra_gap;
        }
    }
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
