//! Ready-made widgets built from [`Node`]s and styled by the [`Theme`].

use crate::color::Color;
use crate::geometry::Insets;
use crate::node::{Align, Content, CursorIcon, Key, Length, Node, Role};
use crate::scene::Font;
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

/// A single-line text input. `on_focus` is sent when it's clicked.
pub fn text_field<M>(
    theme: &Theme,
    field: &TextField,
    focused: bool,
    key: Key,
    placeholder: &str,
    on_focus: M,
) -> Node<M> {
    let mut node = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(30.0))
        .padding(Insets::symmetric(0.0, 10.0))
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
                placeholder: placeholder.to_owned(),
                color: theme.text,
                placeholder_color: theme.text_muted,
                caret_color: theme.accent,
                size: theme.font_size,
            },
            ..Node::new()
        }
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0)),
    );
    node
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
