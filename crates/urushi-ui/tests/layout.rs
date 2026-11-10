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
        // On glass a secondary button rests on `surface`, so it brightens to `selected`.
        hovered.scene.dump().contains(&theme.selected.to_hex()),
        "hovered button brightens"
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
    // The dialog's own space (between its text and buttons) doesn't close it.
    let card = frame
        .access
        .iter()
        .find(|a| a.label == "Delete thread")
        .unwrap()
        .rect;
    let blank = Point::new(card.x + 4.0, card.y + card.height - 4.0);
    assert_eq!(frame.click(blank), None, "a click inside the card");
    assert_eq!(frame.drop_target(blank), None);
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
fn a_max_height_caps_a_fit_box_which_then_scrolls() {
    let mut text = TextSystem::bundled_only();
    let k = key(&["capped"]);
    let rows = |n: usize| (0..n).map(|_| Node::new().height(Length::Fixed(20.0)));
    let capped = |n: usize| -> Node<Msg> {
        Node::column().child(
            Node::column()
                .width(Length::Fixed(100.0))
                .max_height(50.0)
                .scroll(k, 0.0)
                .children(rows(n)),
        )
    };
    // Short content keeps its own height.
    let frame = build(
        &capped(2),
        Rect::new(0.0, 0.0, 100.0, 400.0),
        &mut text,
        None,
    );
    assert_eq!(frame.scroll_regions[0].rect.height, 40.0);
    // Long content stops at the cap and scrolls.
    let frame = build(
        &capped(10),
        Rect::new(0.0, 0.0, 100.0, 400.0),
        &mut text,
        None,
    );
    let region = frame.scroll_regions[0];
    assert_eq!(region.rect.height, 50.0);
    assert_eq!(region.content_height, 200.0);
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

#[test]
fn positioned_stack_children_stay_on_screen_and_receive_secondary_clicks() {
    let mut text = TextSystem::bundled_only();
    let menu = |at| {
        Node::column()
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(30.0))
            .on_click(Msg::A)
            .at(at)
    };
    let ui = Node::stack()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .on_secondary_click(Msg::B),
        )
        .child(menu(Point::new(10.0, 20.0)))
        .child(menu(Point::new(90.0, 95.0)));
    let frame = build(&ui, Rect::new(0.0, 0.0, 100.0, 100.0), &mut text, None);
    assert_eq!(frame.hits[1].rect, Rect::new(10.0, 20.0, 40.0, 30.0));
    assert_eq!(
        frame.hits[2].rect,
        Rect::new(60.0, 70.0, 40.0, 30.0),
        "moved back inside the window"
    );
    assert_eq!(frame.secondary_click(Point::new(5.0, 5.0)), Some(Msg::B));
    assert_eq!(frame.click(Point::new(15.0, 25.0)), Some(Msg::A));
    assert_eq!(
        frame.secondary_click(Point::new(15.0, 25.0)),
        Some(Msg::B),
        "falls through"
    );
}

#[test]
fn drag_sources_and_drop_targets_are_found_by_position() {
    let mut text = TextSystem::bundled_only();
    let ui = Node::column()
        .width(Length::Fill(1.0))
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(20.0))
                .on_click(Msg::A)
                .on_drag(Msg::Focus),
        )
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(20.0))
                .background(Color::hex(0x000000))
                .hover_background(Color::hex(0xffffff))
                .on_drop(Msg::Backdrop),
        );
    let frame = build(
        &ui,
        Rect::new(0.0, 0.0, 100.0, 100.0),
        &mut text,
        Some(Point::new(5.0, 30.0)),
    );
    assert_eq!(frame.drag_source(Point::new(5.0, 5.0)), Some(Msg::Focus));
    assert_eq!(frame.drag_source(Point::new(5.0, 30.0)), None);
    assert_eq!(
        frame.drop_target(Point::new(5.0, 30.0)),
        Some(Msg::Backdrop)
    );
    // A drop target under the pointer is highlighted.
    let dump = frame.scene.dump();
    assert!(dump.contains("#ffffff"), "{dump}");
}

#[test]
fn a_sliding_panel_moves_with_its_children_and_is_clipped() {
    let mut text = TextSystem::bundled_only();
    let ui = |slide: f32| {
        Node::stack()
            .width(Length::Fill(1.0))
            .height(Length::Fill(1.0))
            .clip()
            .child(
                Node::new()
                    .width(Length::Fill(1.0))
                    .height(Length::Fill(1.0))
                    .on_click(Msg::A),
            )
            .child(
                Node::row()
                    .width(Length::Fill(1.0))
                    .height(Length::Fill(1.0))
                    .background(Color::rgb8(1, 2, 3))
                    .on_click(Msg::Backdrop)
                    .slide_x(slide)
                    .child(
                        Node::new()
                            .width(Length::Fixed(20.0))
                            .height(Length::Fixed(20.0))
                            .on_click(Msg::B),
                    ),
            )
    };
    let bounds = Rect::new(100.0, 0.0, 200.0, 100.0);
    // Half out: the panel and its button move left by half the width, and only the
    // visible part takes clicks.
    let frame = build(&ui(0.5), bounds, &mut text, None);
    assert_eq!(quads(&frame.scene)[0], Rect::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(frame.click(Point::new(250.0, 50.0)), Some(Msg::A));
    assert_eq!(frame.click(Point::new(150.0, 50.0)), Some(Msg::Backdrop));
    assert_eq!(frame.hits[1].rect, Rect::new(100.0, 0.0, 100.0, 100.0));
    assert_eq!(
        frame.click(Point::new(10.0, 10.0)),
        None,
        "outside the clip"
    );
    // In place: the button is at the left edge (rows centre their children).
    let frame = build(&ui(0.0), bounds, &mut text, None);
    assert_eq!(frame.click(Point::new(105.0, 45.0)), Some(Msg::B));
}

#[test]
fn glass_paints_shadow_then_frost_then_fill() {
    let mut text = TextSystem::bundled_only();
    let ui: Node<Msg> = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .align(Align::Start)
        .child(
            Node::new()
                .width(Length::Fixed(100.0))
                .height(Length::Fixed(50.0))
                .radius(10.0)
                .shadow(urushi_ui::Shadow {
                    color: Color::hex(0x000000).with_alpha(0.5),
                    blur: 20.0,
                    offset: 6.0,
                })
                .backdrop_blur(24.0)
                .background(Color::hex(0xffffff).with_alpha(0.1))
                .gradient(Color::hex(0xffffff).with_alpha(0.02)),
        );
    let frame = build(&ui, Rect::new(0.0, 0.0, 400.0, 300.0), &mut text, None);
    assert_eq!(
        frame.scene.dump(),
        "quad 0,6 100x50 #00000080 r20 soft 20\n\
         backdrop 0,0 100x50 r10 blur 24\n\
         quad 0,0 100x50 #ffffff1a r10 to #ffffff05\n"
    );

    // Plain boxes dump exactly as before.
    let plain: Node<Msg> = Node::new()
        .width(Length::Fixed(10.0))
        .height(Length::Fixed(10.0))
        .background(Color::hex(0xffffff));
    let frame = build(&plain, Rect::new(0.0, 0.0, 400.0, 300.0), &mut text, None);
    assert_eq!(frame.scene.dump(), "quad 0,0 400x300 #ffffff r0\n");
}
