//! Ready-made widgets built from [`Node`]s and styled by the [`Theme`].

use crate::color::Color;
use crate::geometry::{Insets, Point};
use crate::node::{Align, Content, CursorIcon, Key, Length, Node, Role};
use crate::scene::Font;
use crate::text::TextSystem;
use crate::textfield::TextField;
use crate::theme::Theme;

/// Button emphasis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// The main action of a dialog.
    Primary,
    /// A normal action.
    Secondary,
    /// A destructive action.
    Danger,
    /// Text-only, for toolbars.
    Ghost,
}

/// A labelled button.
pub fn button<M>(theme: &Theme, label: &str, kind: ButtonKind, on_click: M) -> Node<M> {
    let (bg, hover, fg) = match kind {
        ButtonKind::Primary => (
            theme.accent,
            theme.accent.mix(Color::hex(0xffffff), 0.12),
            theme.on_accent,
        ),
        ButtonKind::Secondary => (theme.surface, theme.hover, theme.text),
        ButtonKind::Danger => (
            theme.danger,
            theme.danger.mix(Color::hex(0xffffff), 0.12),
            theme.on_accent,
        ),
        ButtonKind::Ghost => (Color::TRANSPARENT, theme.hover, theme.text_muted),
    };
    let mut node = Node::row()
        .child(Node::text(label, Font::UiMedium, theme.font_size, fg))
        .padding(Insets::symmetric(6.0, 12.0))
        .background(bg)
        .hover_background(hover)
        .radius(theme.radius)
        .justify(Align::Center)
        .on_click(on_click)
        .role(Role::Button, label);
    if kind == ButtonKind::Secondary {
        node = node.border(1.0, theme.border);
    }
    node
}

/// A small square button showing a glyph, e.g. "+" or "×".
pub fn icon_button<M>(theme: &Theme, glyph: &str, label: &str, on_click: M) -> Node<M> {
    Node::row()
        .child(Node::text(
            glyph,
            Font::UiMedium,
            theme.font_size + 2.0,
            theme.text_muted,
        ))
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .justify(Align::Center)
        .radius(4.0)
        .hover_background(theme.hover)
        .on_click(on_click)
        .role(Role::Button, label)
}

/// A single-line text input. `on_focus` is sent when it's clicked. Where in the text
/// the click landed comes from [`crate::Frame::field_at`] and [`crate::caret_at`].
pub fn text_field<M>(
    theme: &Theme,
    field: &TextField,
    focused: bool,
    key: Key,
    placeholder: &str,
    on_focus: M,
) -> Node<M> {
    field_box(theme, field, focused, (key, placeholder, on_focus), false)
        .height(Length::Fixed(30.0))
        .padding(Insets::symmetric(0.0, 10.0))
}

/// A text input for several lines (e.g. notes), `lines` tall. The text wraps to the
/// box, and it scrolls to keep the caret in view. Enter adding a line is up to the
/// caller (`TextField::newline`).
pub fn text_area<M>(
    theme: &Theme,
    field: &TextField,
    focused: bool,
    (key, placeholder, on_focus): (Key, &str, M),
    lines: u8,
) -> Node<M> {
    let pad = 6.0;
    let height = f32::from(lines) * TextSystem::line_height(theme.font_size) + 2.0 * pad;
    field_box(theme, field, focused, (key, placeholder, on_focus), true)
        .height(Length::Fixed(height))
        .padding(Insets::symmetric(pad, 10.0))
}

/// The box and content shared by [`text_field`] and [`text_area`].
fn field_box<M>(
    theme: &Theme,
    field: &TextField,
    focused: bool,
    (key, placeholder, on_focus): (Key, &str, M),
    multiline: bool,
) -> Node<M> {
    let mut node = Node::row()
        .width(Length::Fill(1.0))
        .background(theme.background)
        .border(1.0, if focused { theme.accent } else { theme.border })
        .radius(theme.radius)
        .key(key)
        .on_click(on_focus)
        .cursor(CursorIcon::Text)
        .role(Role::TextInput, placeholder);
    node.children.push(
        Node {
            content: Content::Field {
                text: field.text().to_owned(),
                caret: focused.then(|| field.caret()),
                selection: field.selection().filter(|_| focused),
                placeholder: placeholder.to_owned(),
                color: theme.text,
                placeholder_color: theme.text_muted,
                caret_color: theme.accent,
                selection_color: theme.accent.with_alpha(0.35),
                size: theme.font_size,
                multiline,
            },
            ..Node::new()
        }
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0)),
    );
    node
}

/// Text wrapped to its box, showing as many lines as fit; when there's more, the last
/// line shown ends with an ellipsis. Line breaks in `text` start new lines. Give it a
/// fixed or filling height: it doesn't grow to fit.
pub fn wrapped_text<M>(text: &str, font: Font, size: f32, color: Color) -> Node<M> {
    let text = text.to_owned();
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .paint(move |rect, scene, ts| {
            let line = TextSystem::line_height(size);
            let lines = ts.wrap(&text, font, size, rect.width);
            let fit = (rect.height / line).floor().max(0.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a small non-negative line count"
            )]
            let fit = fit as usize;
            scene.push_clip(rect);
            for (row, range) in lines.iter().take(fit).enumerate() {
                let more = row + 1 == fit && lines.len() > fit;
                let shown = if more {
                    // The rest of the text on one line, cut short with an ellipsis.
                    let rest = text[range.start..].replace('\n', " ");
                    ts.truncate(rest.trim_end(), font, size, rect.width)
                } else {
                    text[range.clone()].trim_end().to_owned()
                };
                #[allow(clippy::cast_precision_loss, reason = "a few lines")]
                let y = rect.y + row as f32 * line;
                scene.text(Point::new(rect.x, y.round()), shown, font, size, color);
            }
            scene.pop_clip();
        })
}

/// A labelled on/off switch.
pub fn toggle<M>(theme: &Theme, on: bool, label: &str, on_click: M) -> Node<M> {
    let track = if on { theme.accent } else { theme.selected };
    let knob = Node::new()
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .radius(7.0)
        .background(Color::hex(0xffffff));
    let switch = Node::row()
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(18.0))
        .padding(Insets::all(2.0))
        .radius(9.0)
        .background(track)
        .justify(if on { Align::End } else { Align::Start })
        .child(knob);
    Node::row()
        .gap(10.0)
        .child(switch)
        .child(Node::text(label, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .on_click(on_click)
        .role(
            Role::Switch,
            format!("{label}: {}", if on { "on" } else { "off" }),
        )
}

/// A one-pixel divider.
pub fn divider<M>(theme: &Theme, vertical: bool) -> Node<M> {
    let node = Node::new().background(theme.border);
    if vertical {
        node.width(Length::Fixed(1.0)).height(Length::Fill(1.0))
    } else {
        node.height(Length::Fixed(1.0)).width(Length::Fill(1.0))
    }
}

/// A centred dialog over a dimmed backdrop. `on_dismiss` is sent when the backdrop is
/// clicked (pass `None` to make the dialog modal-only).
pub fn modal<M: Clone>(
    theme: &Theme,
    title: &str,
    width: f32,
    body: Node<M>,
    actions: Vec<Node<M>>,
    on_dismiss: Option<M>,
) -> Node<M> {
    let header = Node::text(title, Font::UiSemibold, theme.font_size + 2.0, theme.text);
    let footer = Node::row()
        .gap(8.0)
        .justify(Align::End)
        .width(Length::Fill(1.0))
        .children(actions);
    let card = Node::column()
        .width(Length::Fixed(width))
        .padding(Insets::all(20.0))
        .gap(16.0)
        .background(theme.surface)
        .border(1.0, theme.border)
        .radius(theme.radius * 2.0)
        .key(crate::node::key(&["modal", title]))
        .role(Role::Dialog, title)
        .child(header)
        .child(body)
        .child(footer);
    let mut backdrop = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .background(theme.scrim)
        .justify(Align::Center)
        .align(Align::Center)
        .child(card);
    if let Some(dismiss) = on_dismiss {
        backdrop = backdrop.on_click(dismiss);
        backdrop.cursor = CursorIcon::Default;
    }
    backdrop
}

/// Height of one row in a selector's option list.
const SELECT_ROW: f32 = 28.0;

/// A selector's button: the chosen value and a chevron. Sends `on_toggle` when clicked;
/// show [`select_options`] under it while `open`.
pub fn select_button<M>(theme: &Theme, value: &str, open: bool, key: Key, on_toggle: M) -> Node<M> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(30.0))
        .padding(Insets::symmetric(0.0, 10.0))
        .gap(8.0)
        .background(theme.background)
        .hover_background(theme.hover)
        .border(1.0, if open { theme.accent } else { theme.border })
        .radius(theme.radius)
        .key(key)
        .on_click(on_toggle)
        .role(Role::Button, value)
        .child(Node::text(value, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .child(Node::text(
            if open { "▴" } else { "▾" },
            Font::Ui,
            theme.font_size,
            theme.text_muted,
        ))
}

/// One option of a selector: its label, whether it's the current choice, and the
/// message that chooses it.
pub struct SelectOption<M> {
    /// Shown text.
    pub label: String,
    /// The current choice (shown with a check mark).
    pub selected: bool,
    /// Sent when chosen.
    pub on_pick: M,
}

/// The option list of an open selector. Longer than `max_visible` rows, it scrolls
/// (`scroll` is its key and current offset).
pub fn select_options<M>(
    theme: &Theme,
    options: Vec<SelectOption<M>>,
    scroll: (Key, f32),
    max_visible: usize,
) -> Node<M> {
    let rows = options.len();
    let mut list = Node::column()
        .width(Length::Fill(1.0))
        .padding(Insets::all(4.0))
        .background(theme.surface)
        .border(1.0, theme.border)
        .radius(theme.radius)
        .clip();
    if rows > max_visible {
        #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
        let height = max_visible as f32 * SELECT_ROW + 8.0;
        list = list
            .height(Length::Fixed(height))
            .scroll(scroll.0, scroll.1);
    }
    list.children(options.into_iter().map(|o| {
        Node::row()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(SELECT_ROW))
            .padding(Insets::symmetric(0.0, 8.0))
            .gap(8.0)
            .radius(theme.radius)
            .background(if o.selected {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(theme.hover)
            .on_click(o.on_pick)
            .role(Role::Item, o.label.clone())
            .child(
                Node::text(
                    if o.selected { "✓" } else { "" },
                    Font::Ui,
                    theme.font_size,
                    theme.accent,
                )
                .width(Length::Fixed(12.0)),
            )
            .child(
                Node::text(o.label, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)),
            )
    }))
}
