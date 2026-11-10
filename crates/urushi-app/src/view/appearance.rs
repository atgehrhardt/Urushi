//! Settings' **Appearance** section: the color schemes as small previews of the
//! backdrop (its fill and three glows), **Custom** with a `#rrggbb` field per color,
//! and how the logo is drawn. Changes show at once and are kept on Save.

use urushi_core::{LogoStyle, Scheme};
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::dialogs::{field, label};
use crate::appearance::{COLOR_NAMES, LookDraft};
use crate::msg::Msg;
use crate::state::{AppState, Field};

/// Width and height of a scheme's preview.
const TILE: (f32, f32) = (72.0, 44.0);

/// The section, for a daemon that keeps an appearance.
pub(super) fn section(state: &AppState, theme: &Theme, look: &LookDraft) -> Vec<Node<Msg>> {
    let current = look.appearance();
    let mut tiles: Vec<Node<Msg>> = Scheme::PRESETS
        .into_iter()
        .map(|s| tile(theme, s, !look.custom && look.preset == s, Some(s)))
        .collect();
    tiles.push(tile(theme, current.scheme, look.custom, None));
    let mut out = vec![
        label(theme, "Appearance"),
        Node::row().gap(8.0).children(tiles),
    ];
    if look.custom {
        for (i, name) in COLOR_NAMES.iter().enumerate() {
            let Ok(i) = u8::try_from(i) else { break };
            let shown = urushi_core::Rgb::parse(look.hex[usize::from(i)].text());
            out.push(
                Node::row()
                    .width(Length::Fill(1.0))
                    .gap(8.0)
                    .align(Align::Center)
                    .child(
                        Node::text(*name, Font::Ui, theme.font_size_small, theme.text_muted)
                            .width(Length::Fixed(84.0)),
                    )
                    .child(
                        Node::new()
                            .width(Length::Fixed(18.0))
                            .height(Length::Fixed(18.0))
                            .radius(9.0)
                            .border(1.0, theme.border)
                            .background(
                                shown.map_or(Color::TRANSPARENT, |c| Color::hex(c.value())),
                            ),
                    )
                    .child(
                        field(
                            state,
                            theme,
                            &look.hex[usize::from(i)],
                            Field::LookColor(i),
                            "#rrggbb",
                        )
                        .width(Length::Fill(1.0)),
                    ),
            );
        }
    }
    out.push(label(theme, "Logo"));
    out.push(
        Node::row()
            .gap(6.0)
            .children(LogoStyle::ALL.into_iter().map(|style| {
                let on = look.logo == style;
                Node::row()
                    .padding(Insets::symmetric(4.0, 10.0))
                    .radius(theme.radius)
                    .border(1.0, if on { theme.accent } else { theme.border })
                    .hover_background(theme.hover)
                    .key(key(&["logo-style", style.name()]))
                    .on_click(Msg::LookLogo(style))
                    .role(Role::Switch, format!("Logo: {}", style.name()))
                    .child(Node::text(
                        style.name(),
                        Font::UiMedium,
                        theme.font_size_small,
                        if on { theme.text } else { theme.text_muted },
                    ))
            })),
    );
    out
}

/// A scheme's preview: its fill with its glows as soft dots, and its name.
fn tile(theme: &Theme, scheme: Scheme, chosen: bool, pick: Option<Scheme>) -> Node<Msg> {
    let c = scheme.colors();
    let name = if pick.is_some() {
        scheme.name()
    } else {
        "Custom"
    };
    let glow = |rgb: urushi_core::Rgb, size: f32| {
        Node::new()
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .radius(size / 2.0)
            .background(Color::hex(rgb.value()).with_alpha(0.85))
    };
    let preview = Node::row()
        .width(Length::Fixed(TILE.0))
        .height(Length::Fixed(TILE.1))
        .radius(theme.radius)
        .background(Color::hex(c.background.value()))
        .border(
            if chosen { 2.0 } else { 1.0 },
            if chosen {
                Color::hex(c.accent.value())
            } else {
                theme.border
            },
        )
        .padding(Insets::symmetric(0.0, 8.0))
        .gap(4.0)
        .align(Align::Center)
        .child(glow(c.glows[0], 16.0))
        .child(glow(c.glows[2], 10.0))
        .child(glow(c.glows[1], 14.0));
    Node::column()
        .gap(4.0)
        .align(Align::Center)
        .key(key(&["scheme", name]))
        .on_click(Msg::LookScheme(pick))
        .role(Role::Switch, format!("Colors: {name}"))
        .child(preview)
        .child(Node::text(
            name,
            Font::UiMedium,
            theme.font_size_small,
            if chosen { theme.text } else { theme.text_muted },
        ))
}
