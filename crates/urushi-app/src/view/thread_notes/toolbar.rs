//! The notes editor's formatting controls: one small button per Markdown mark or line
//! marker, each showing what it makes (B in bold, I in italic, S struck through, …).

use urushi_ui::{
    Align, Color, Font, Length, Node, Point, Role, Run, Styled, TextStyle, TextSystem, Theme, key,
};

use crate::msg::{Msg, NoteFormat, ThreadMsg};

/// Size of a button.
const BUTTON: f32 = 26.0;

/// The row of formatting buttons.
pub(super) fn toolbar(theme: &Theme) -> Node<Msg> {
    let shortcut = |k: &str| {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            format!(" (⌘{k})")
        } else {
            format!(" (Ctrl+{k})")
        }
    };
    let plain = TextStyle::default();
    let buttons = [
        (
            NoteFormat::Bold,
            "B",
            TextStyle {
                font: Font::UiSemibold,
                ..plain
            },
            format!("Bold{}", shortcut("B")),
        ),
        (
            NoteFormat::Italic,
            "I",
            TextStyle {
                font: Font::UiItalic,
                ..plain
            },
            format!("Italic{}", shortcut("I")),
        ),
        (
            NoteFormat::Strike,
            "S",
            TextStyle {
                strike: true,
                ..plain
            },
            format!("Strikethrough{}", shortcut("Shift+X")),
        ),
        (
            NoteFormat::Code,
            "<>",
            TextStyle {
                font: Font::MONO,
                ..plain
            },
            format!("Code{}", shortcut("E")),
        ),
        (
            NoteFormat::Heading,
            "H",
            TextStyle {
                font: Font::UiSemibold,
                ..plain
            },
            "Heading".to_owned(),
        ),
        (NoteFormat::Bullet, "•", plain, "Bulleted list".to_owned()),
        (
            NoteFormat::Numbered,
            "1.",
            plain,
            "Numbered list".to_owned(),
        ),
        (NoteFormat::Task, "", plain, "Checklist".to_owned()),
        (
            NoteFormat::Link,
            "↗",
            plain,
            format!("Link{}", shortcut("K")),
        ),
    ];
    Node::row()
        .gap(2.0)
        .align(Align::Center)
        .children(buttons.into_iter().map(|(format, glyph, style, label)| {
            let face = if format == NoteFormat::Task {
                check_box(theme)
            } else {
                glyph_node(glyph, style, theme)
            };
            Node::row()
                .width(Length::Fixed(BUTTON))
                .height(Length::Fixed(BUTTON))
                .justify(Align::Center)
                .align(Align::Center)
                .radius(theme.radius)
                .hover_background(theme.hover)
                .key(key(&["notes-format", &format!("{format:?}")]))
                .on_click(Msg::Thread(ThreadMsg::FormatNotes(format)))
                .role(Role::Button, label)
                .child(face)
        }))
}

/// `glyph` drawn centred in `style`, in the muted text color.
fn glyph_node(glyph: &str, style: TextStyle, theme: &Theme) -> Node<Msg> {
    let glyph = glyph.to_owned();
    let size = theme.font_size;
    let color = theme.text_muted;
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(TextSystem::line_height(size)))
        .paint(move |rect, scene, ts| {
            let runs = [Run::new(0..glyph.len(), style)];
            let styled = Styled::new(&glyph, &runs);
            let w = styled.width(ts, 0..glyph.len(), size);
            let at = Point::new((rect.x + (rect.width - w) / 2.0).round(), rect.y);
            styled.paint(scene, ts, at, (0..glyph.len(), size, color));
        })
}

/// An empty check box, drawn (the bundled fonts have no ballot box).
fn check_box(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fixed(11.0))
        .height(Length::Fixed(11.0))
        .radius(2.0)
        .border(1.25, theme.text_muted)
        .background(Color::TRANSPARENT)
}
