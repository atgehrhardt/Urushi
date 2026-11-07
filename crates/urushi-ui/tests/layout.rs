//! Layout and painting, tested through the scene and hit regions (no GPU).
#![allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use urushi_ui::widgets::{ButtonKind, button, modal, text_field, toggle};
use urushi_ui::{
    Align, Color, Font, Insets, Length, Node, Point, Primitive, Rect, TextField, TextSystem, Theme,
    build, key,
};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    A,
    B,
    Backdrop,
    Focus,
}

fn quads(scene: &urushi_ui::Scene) -> Vec<Rect> {
    scene
        .primitives
        .iter()
        .filter_map(|p| {
            if let Primitive::Quad(q) = p {
                Some(q.rect)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn fixed_fill_and_fit_share_a_row() {
    let mut text = TextSystem::bundled_only();
    let red = Color::hex(0xff0000);
    let root: Node<Msg> = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::all(10.0))
        .gap(10.0)
        .child(
            Node::new()
                .width(Length::Fixed(100.0))
                .height(Length::Fill(1.0))
                .background(red),
        )
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .background(red),
        )
        .child(
            Node::new()
                .width(Length::Fill(3.0))
                .height(Length::Fixed(20.0))
                .background(red),
        );
    let frame = build(&root, Rect::new(0.0, 0.0, 530.0, 100.0), &mut text, None);
    let q = quads(&frame.scene);
    assert_eq!(q[0], Rect::new(10.0, 10.0, 100.0, 80.0));
    // 530 - 20 padding - 100 fixed - 20 gaps = 390 → 1:3 split.
    assert_eq!(q[1], Rect::new(120.0, 10.0, 97.5, 80.0));
    assert_eq!(
        q[2],
        Rect::new(227.5, 40.0, 292.5, 20.0),
        "row children are centred on the cross axis"
    );
}

#[test]
fn column_stretches_and_justifies() {
    let mut text = TextSystem::bundled_only();
    let root: Node<Msg> = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::End)
        .child(
            Node::new()
                .height(Length::Fixed(30.0))
                .background(Color::hex(0x00ff00)),
        );
    let frame = build(&root, Rect::new(0.0, 0.0, 200.0, 100.0), &mut text, None);
    assert_eq!(quads(&frame.scene)[0], Rect::new(0.0, 70.0, 200.0, 30.0));
}

#[test]
fn clicks_hit_the_topmost_target_and_hover_changes_fill() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let ui = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(8.0)
        .child(button(&theme, "First", ButtonKind::Secondary, Msg::A))
        .child(button(&theme, "Second", ButtonKind::Primary, Msg::B));
    let bounds = Rect::new(0.0, 0.0, 400.0, 40.0);
    let frame = build(&ui, bounds, &mut text, None);
    let first = frame
        .hits
        .iter()
        .find(|h| h.on_click == Some(Msg::A))
        .unwrap()
        .rect;
    let second = frame
        .hits
        .iter()
        .find(|h| h.on_click == Some(Msg::B))
        .unwrap()
        .rect;
    assert!(first.right() < second.x);
    assert_eq!(frame.click(first.center()), Some(Msg::A));
    assert_eq!(frame.click(second.center()), Some(Msg::B));
    assert_eq!(frame.click(Point::new(399.0, 39.0)), None);

    let hovered = build(&ui, bounds, &mut text, Some(first.center()));
    assert!(
        hovered.scene.dump().contains(&theme.hover.to_hex()),
        "hovered button uses the hover colour"
    );
    assert_eq!(
        hovered.cursor(first.center()),
        urushi_ui::CursorIcon::Pointer
    );
}

#[test]
fn modal_covers_the_app_and_dismisses_on_backdrop() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let dialog = modal(
        &theme,
        "Delete thread",
        360.0,
        Node::text("This cannot be undone.", Font::Ui, 13.0, theme.text),
        vec![
            button(&theme, "Cancel", ButtonKind::Secondary, Msg::A),
            button(&theme, "Delete", ButtonKind::Danger, Msg::B),
        ],
        Some(Msg::Backdrop),
    );
    let frame = build(&dialog, Rect::new(0.0, 0.0, 800.0, 600.0), &mut text, None);
    assert_eq!(frame.click(Point::new(5.0, 5.0)), Some(Msg::Backdrop));
    let delete = frame
        .hits
        .iter()
        .find(|h| h.on_click == Some(Msg::B))
        .unwrap();
    assert_eq!(frame.click(delete.rect.center()), Some(Msg::B));
    let dump = frame.scene.dump();
    assert!(dump.contains("\"Delete thread\""));
    assert!(dump.contains(&theme.danger.to_hex()));
    assert!(
        frame
            .access
            .iter()
            .any(|a| a.label == "Delete" && a.clickable)
    );
}

#[test]
fn labels_truncate_to_fit() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let ui: Node<Msg> = Node::row().width(Length::Fixed(80.0)).child(
        Node::text(
            "A very long thread title that overflows",
            Font::Ui,
            13.0,
            theme.text,
        )
        .width(Length::Fill(1.0)),
    );
    let frame = build(&ui, Rect::new(0.0, 0.0, 80.0, 20.0), &mut text, None);
    let shown = frame
        .scene
        .primitives
        .iter()
        .find_map(|p| {
            if let Primitive::Text(t) = p {
                Some(t.text.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(shown.ends_with('…'), "{shown}");
}

#[test]
fn scroll_regions_clamp_and_offset_children() {
    let mut text = TextSystem::bundled_only();
    let k = key(&["list"]);
    let items = (0..10).map(|_| {
        Node::new()
            .height(Length::Fixed(20.0))
            .background(Color::hex(0x0000ff))
    });
    let ui: Node<Msg> = Node::column()
        .width(Length::Fixed(100.0))
        .height(Length::Fixed(50.0))
        .scroll(k, 1000.0)
        .children(items);
    let frame = build(&ui, Rect::new(0.0, 0.0, 100.0, 50.0), &mut text, None);
    let region = frame.scroll_region(Point::new(10.0, 10.0)).unwrap();
    assert_eq!(region.content_height, 200.0);
    assert_eq!(region.offset, 150.0, "clamped to content - viewport");
    assert_eq!(quads(&frame.scene)[0].y, -150.0);
    assert!(frame.scene.dump().starts_with("clip 0,0 100x50"));
}

#[test]
fn text_field_shows_caret_only_when_focused_and_toggle_reports_state() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let field = TextField::new("abc");
    let k = key(&["field"]);
    let focused = build(
        &text_field(&theme, &field, true, k, "Title", Msg::Focus),
        Rect::new(0.0, 0.0, 200.0, 30.0),
        &mut text,
        None,
    );
    let unfocused = build(
        &text_field(&theme, &field, false, k, "Title", Msg::Focus),
        Rect::new(0.0, 0.0, 200.0, 30.0),
        &mut text,
        None,
    );
    let caret = theme.accent.to_hex();
    assert!(
        focused.scene.dump().matches(&caret).count()
            > unfocused.scene.dump().matches(&caret).count()
    );
    let on = build(
        &toggle(&theme, true, "Serve on tailnet", Msg::A),
        Rect::new(0.0, 0.0, 300.0, 24.0),
        &mut text,
        None,
    );
    assert!(on.access.iter().any(|a| a.label == "Serve on tailnet: on"));
}

#[test]
fn scene_dump_is_stable() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let ui = Node::row().padding(Insets::all(4.0)).child(button(
        &theme,
        "OK",
        ButtonKind::Primary,
        Msg::A,
    ));
    let a = build(&ui, Rect::new(0.0, 0.0, 120.0, 40.0), &mut text, None)
        .scene
        .dump();
    let b = build(&ui, Rect::new(0.0, 0.0, 120.0, 40.0), &mut text, None)
        .scene
        .dump();
    assert_eq!(a, b);
    assert!(
        a.contains("text") && a.contains("\"OK\" ui-medium 13"),
        "{a}"
    );
}

#[test]
fn stack_layers_children_and_topmost_wins() {
    let mut text = TextSystem::bundled_only();
    let ui = Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .on_click(Msg::A),
        )
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .on_click(Msg::B),
        );
    let frame = build(&ui, Rect::new(0.0, 0.0, 100.0, 100.0), &mut text, None);
    assert_eq!(frame.click(Point::new(50.0, 50.0)), Some(Msg::B));
    assert_eq!(frame.hits[0].rect, frame.hits[1].rect);
}
