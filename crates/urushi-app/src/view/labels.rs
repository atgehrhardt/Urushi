//! How board labels look: the palette colors of cards and columns, swatches to pick one,
//! and tag chips (each tag in the color its name gives it, `urushi_core::tag_color`).
//! Colors arrive as wire `LabelColor` values (0: none); the theme picks the shades.

use urushi_core::LabelColor;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;

/// Size of a color swatch in the editor.
const SWATCH: f32 = 18.0;

/// The palette colors with their wire values, in order.
pub(super) fn palette() -> impl Iterator<Item = (LabelColor, i32)> {
    LabelColor::ALL
        .into_iter()
        .map(|c| (c, urushi_proto::convert::color_to_proto(Some(c))))
}

/// The theme's shade of a wire color; `None` for no color (or one this app doesn't know).
pub(super) fn color(theme: &Theme, wire: i32) -> Option<Color> {
    urushi_proto::convert::color_from_proto(wire).map(|c| theme.labels[c.index()])
}

/// The name of a wire color ("Red"), or "None".
pub(super) fn color_name(wire: i32) -> String {
    urushi_proto::convert::color_from_proto(wire).map_or_else(
        || "None".to_owned(),
        |c| {
            let name = c.name();
            name[..1].to_uppercase() + &name[1..]
        },
    )
}

/// The color a tag is shown in.
pub(super) fn tag_color(theme: &Theme, tag: &str) -> Color {
    theme.labels[urushi_core::board::tag_color(tag).index()]
}

/// A small round dot of `color` (or an empty ring for none), for menus.
pub(super) fn dot(theme: &Theme, color: Option<Color>, size: f32) -> Node<Msg> {
    let node = Node::new()
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .radius(size / 2.0);
    match color {
        Some(c) => node.background(c),
        None => node.border(1.0, theme.text_muted),
    }
}

/// A tag chip. With `on_remove` it has a × that sends it.
pub(super) fn tag_chip(theme: &Theme, tag: &str, size: f32, on_remove: Option<Msg>) -> Node<Msg> {
    let c = tag_color(theme, tag);
    let mut chip = Node::row()
        .padding(Insets::symmetric(1.0, 6.0))
        .gap(4.0)
        .radius(4.0)
        .align(Align::Center)
        .background(c.with_alpha(0.2))
        .child(Node::text(tag, Font::UiMedium, size, c));
    if let Some(msg) = on_remove {
        chip = chip.child(
            Node::text("×", Font::UiMedium, size + 1.0, c)
                .key(key(&["tag-remove", tag]))
                .on_click(msg)
                .role(Role::Button, format!("Remove tag {tag}")),
        );
    }
    chip
}

/// A row of swatches, "none" first; the chosen one is ringed. `pick` makes the message
/// for a wire color.
pub(super) fn swatches(theme: &Theme, chosen: i32, pick: impl Fn(i32) -> Msg) -> Node<Msg> {
    let swatch = |wire: i32, fill: Option<Color>, name: String| {
        let on = wire == chosen;
        let mut inner = Node::row()
            .width(Length::Fixed(SWATCH))
            .height(Length::Fixed(SWATCH))
            .radius(SWATCH / 2.0)
            .justify(Align::Center)
            .align(Align::Center);
        inner = match fill {
            Some(c) => inner.background(c),
            None => inner.border(1.0, theme.text_muted).child(Node::text(
                "∅",
                Font::Ui,
                theme.font_size_small,
                theme.text_muted,
            )),
        };
        Node::row()
            .padding(Insets::all(2.0))
            .radius(SWATCH / 2.0 + 2.0)
            .border(1.5, if on { theme.text } else { Color::TRANSPARENT })
            .key(key(&["swatch", &name]))
            .on_click(pick(wire))
            .role(Role::Item, format!("Color {name}"))
            .child(inner)
    };
    Node::row()
        .gap(4.0)
        .align(Align::Center)
        .child(swatch(0, None, "None".into()))
        .children(
            palette()
                .map(|(c, wire)| swatch(wire, Some(theme.labels[c.index()]), color_name(wire))),
        )
}

/// About how wide `text` is drawn at `size`, for wrapping chips into rows (the layout
/// doesn't wrap rows itself). Errs wide, so a row never overflows.
pub(super) fn text_width(text: &str, size: f32) -> f32 {
    #[allow(clippy::cast_precision_loss, reason = "short labels")]
    let chars = text.chars().count() as f32;
    chars * size * 0.62
}

/// `items` (each with its approximate width) in rows no wider than `max`.
pub(super) fn wrap(items: Vec<(f32, Node<Msg>)>, gap: f32, max: f32) -> Node<Msg> {
    let mut rows: Vec<Node<Msg>> = Vec::new();
    let mut row = Node::row().gap(gap);
    let mut used = 0.0;
    let mut empty = true;
    for (width, item) in items {
        if !empty && used + gap + width > max {
            rows.push(std::mem::replace(&mut row, Node::row().gap(gap)));
            used = 0.0;
            empty = true;
        }
        used += if empty { width } else { gap + width };
        row = row.child(item);
        empty = false;
    }
    if !empty {
        rows.push(row);
    }
    Node::column().gap(gap).children(rows)
}

/// A thumbnail scaled to fit a `side` × `side` box (keeping its shape), centred in it.
pub(super) fn thumbnail_box(
    theme: &Theme,
    image: Option<&std::sync::Arc<urushi_ui::ImageData>>,
    side: f32,
) -> Node<Msg> {
    let frame = Node::row()
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .radius(4.0)
        .clip()
        .justify(Align::Center)
        .align(Align::Center)
        .background(theme.background);
    let Some(image) = image else {
        return frame;
    };
    #[allow(clippy::cast_precision_loss, reason = "thumbnails are small")]
    let (w, h) = (image.width as f32, image.height as f32);
    let scale = side / w.max(h);
    frame.child(Node::image(
        image.clone(),
        (w * scale).round(),
        (h * scale).round(),
    ))
}

/// "1 file", "3 files".
pub(super) fn files_label(n: usize) -> String {
    if n == 1 {
        "1 file".to_owned()
    } else {
        format!("{n} files")
    }
}

/// The approximate width of a [`tag_chip`].
pub(super) fn tag_chip_width(tag: &str, size: f32, removable: bool) -> f32 {
    text_width(tag, size) + 12.0 + if removable { 4.0 + size } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_wrap_into_rows_that_fit() {
        let item = |w: f32| (w, Node::<Msg>::new().width(Length::Fixed(w)));
        let rows = wrap(
            vec![item(50.0), item(50.0), item(50.0), item(120.0)],
            5.0,
            160.0,
        );
        let counts: Vec<usize> = rows.children.iter().map(|r| r.children.len()).collect();
        // 50 + 5 + 50 + 5 + 50 = 160 fits; 120 more doesn't.
        assert_eq!(counts, [3, 1]);
        assert!(wrap(Vec::new(), 5.0, 100.0).children.is_empty());
    }

    #[test]
    fn wire_colors_map_to_the_theme_palette_and_names() {
        let theme = Theme::dark();
        assert_eq!(color(&theme, 0), None);
        assert_eq!(color(&theme, 999), None);
        for (c, wire) in palette() {
            assert_eq!(color(&theme, wire), Some(theme.labels[c.index()]));
        }
        assert_eq!(color_name(0), "None");
        assert_eq!(color_name(1), "Red");
    }
}
