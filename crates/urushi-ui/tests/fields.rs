//! Text fields as painted (selection, sideways scrolling, wrapping) and clicked
//! (pointer → caret), wrapped text, and bitmaps in the scene. No GPU.
#![allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::sync::Arc;

use urushi_ui::widgets::{text_area, text_field, wrapped_text};
use urushi_ui::{
    Font, ImageData, Length, Node, Point, Primitive, Rect, TextField, TextSystem, Theme, build,
    caret_at, key,
};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Focus,
}

fn field_frame(
    field: &TextField,
    focused: bool,
    width: f32,
    text: &mut TextSystem,
) -> urushi_ui::Frame<Msg> {
    let theme = Theme::dark();
    let root = Node::column().child(
        text_field(&theme, field, focused, key(&["f"]), "Type here", Msg::Focus)
            .width(Length::Fixed(width)),
    );
    build(&root, Rect::new(0.0, 0.0, 800.0, 100.0), text, None)
}

/// A point at `x` on a single-line field's line.
fn at(x: f32) -> Point {
    Point::new(x, 15.0)
}

#[test]
fn a_click_maps_to_the_nearest_character_boundary() {
    let mut text = TextSystem::bundled_only();
    let field = TextField::new("hello world");
    let frame = field_frame(&field, false, 300.0, &mut text);
    let region = frame.field_at(Point::new(20.0, 15.0)).cloned().unwrap();
    assert_eq!(region.key, Some(key(&["f"])), "found by its widget's key");
    assert_eq!(frame.field(key(&["f"])), Some(&region));
    let x_of =
        |text: &mut TextSystem, s: &str| region.origin_x + text.measure(s, Font::Ui, region.size);
    assert_eq!(caret_at(&mut text, &region, at(region.origin_x - 5.0)), 0);
    let after_hello = x_of(&mut text, "hello");
    assert_eq!(caret_at(&mut text, &region, at(after_hello + 0.5)), 5);
    assert_eq!(caret_at(&mut text, &region, at(after_hello - 0.5)), 5);
    assert_eq!(caret_at(&mut text, &region, at(299.0)), 11, "past the end");
    // Just right of the start of "w" (after "hello ") still lands before it.
    let before_w = x_of(&mut text, "hello ");
    assert_eq!(caret_at(&mut text, &region, at(before_w + 1.0)), 6);
    assert!(frame.field_at(Point::new(500.0, 15.0)).is_none());
}

#[test]
fn a_hidden_field_is_not_clickable() {
    let mut text = TextSystem::bundled_only();
    let theme = Theme::dark();
    let field = TextField::new("under");
    // A field covered by a later, clickable node.
    let root = Node::stack()
        .child(text_field(
            &theme,
            &field,
            false,
            key(&["f"]),
            "",
            Msg::Focus,
        ))
        .child(
            Node::new()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .on_click(Msg::Focus),
        );
    let frame = build(&root, Rect::new(0.0, 0.0, 300.0, 30.0), &mut text, None);
    assert!(frame.field_at(Point::new(20.0, 15.0)).is_none());
}

#[test]
fn long_text_scrolls_to_keep_the_caret_in_view() {
    let mut text = TextSystem::bundled_only();
    let long = "a fairly long line of text that will not fit in a narrow field at all";
    let field = TextField::new(long);
    let focused = field_frame(&field, true, 120.0, &mut text);
    let region = &focused.fields[0];
    assert!(region.origin_x < region.rect.x, "scrolled left: {region:?}");
    // The caret (at the end) is inside the box.
    let caret_x = region.origin_x + text.measure(long, Font::Ui, region.size);
    assert!(caret_x <= region.rect.x + region.rect.width);
    // Unfocused, it shows from the start again.
    let unfocused = field_frame(&field, false, 120.0, &mut text);
    assert_eq!(unfocused.fields[0].origin_x, unfocused.fields[0].rect.x);
}

#[test]
fn the_selection_is_painted_behind_the_text() {
    let mut text = TextSystem::bundled_only();
    let theme = Theme::dark();
    let mut field = TextField::new("pick me");
    field.select_all();
    let frame = field_frame(&field, true, 300.0, &mut text);
    let selection = theme.accent.with_alpha(0.35);
    let primitives = &frame.scene.primitives;
    let highlight = primitives
        .iter()
        .position(|p| matches!(p, Primitive::Quad(q) if q.color == selection))
        .expect("a selection quad");
    let label = primitives
        .iter()
        .position(|p| matches!(p, Primitive::Text(t) if t.text == "pick me"))
        .unwrap();
    assert!(highlight < label);
    // Not focused: no highlight.
    let frame = field_frame(&field, false, 300.0, &mut text);
    assert!(
        !frame
            .scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Quad(q) if q.color == selection))
    );
}

#[test]
fn images_are_drawn_over_their_box() {
    let mut text = TextSystem::bundled_only();
    let data = Arc::new(ImageData::new(42, 2, 2, vec![255; 16]).unwrap());
    let root: Node<Msg> = Node::row().child(Node::image(data, 24.0, 24.0));
    let frame = build(&root, Rect::new(10.0, 10.0, 200.0, 50.0), &mut text, None);
    // Rows centre their children: (50 - 24) / 2 below the top.
    assert_eq!(frame.scene.dump(), "image 10,23 24x24 #42\n");
}

fn area_frame(field: &TextField, focused: bool, text: &mut TextSystem) -> urushi_ui::Frame<Msg> {
    let theme = Theme::dark();
    let root = Node::column().child(
        text_area(
            &theme,
            field,
            focused,
            (key(&["a"]), "Notes", Msg::Focus),
            3,
        )
        .width(Length::Fixed(200.0)),
    );
    build(&root, Rect::new(0.0, 0.0, 800.0, 200.0), text, None)
}

fn texts(frame: &urushi_ui::Frame<Msg>) -> Vec<String> {
    frame
        .scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            Primitive::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_text_area_wraps_and_maps_clicks_to_lines() {
    let mut text = TextSystem::bundled_only();
    let field = TextField::multiline("first\nsecond line that is long enough to wrap around");
    let frame = area_frame(&field, false, &mut text);
    let region = frame.fields[0].clone();
    assert!(region.lines.len() >= 3, "a break and a wrap: {region:?}");
    let shown = texts(&frame);
    assert_eq!(shown[0], "first");
    assert!(shown[1].starts_with("second"), "{shown:?}");
    let line = TextSystem::line_height(region.size);
    let row = |n: f32| region.origin_y + line * n + line / 2.0;
    let left = region.origin_x - 3.0;
    assert_eq!(caret_at(&mut text, &region, Point::new(left, row(0.0))), 0);
    assert_eq!(caret_at(&mut text, &region, Point::new(left, row(1.0))), 6);
    // Past the end of the soft-wrapped line: before its trailing space, on that line.
    let end = region.lines[1].end;
    let caret = caret_at(&mut text, &region, Point::new(500.0, row(1.0)));
    assert!(caret < end && caret > 6, "{caret} vs {end}");
    // Above and below the text: the first and last lines.
    assert_eq!(caret_at(&mut text, &region, Point::new(left, -50.0)), 0);
    let last = region.lines.last().unwrap().start;
    assert_eq!(caret_at(&mut text, &region, Point::new(left, 500.0)), last);
}

#[test]
fn a_text_area_scrolls_to_keep_the_caret_in_view() {
    let mut text = TextSystem::bundled_only();
    let field = TextField::multiline("1\n2\n3\n4\n5\n6");
    let focused = area_frame(&field, true, &mut text);
    let region = &focused.fields[0];
    let line = TextSystem::line_height(region.size);
    assert!(region.origin_y < region.rect.y, "scrolled up: {region:?}");
    assert!(region.origin_y + 6.0 * line <= region.rect.y + region.rect.height + 0.5);
    let unfocused = area_frame(&field, false, &mut text);
    assert_eq!(unfocused.fields[0].origin_y, unfocused.fields[0].rect.y);
    assert!(
        !texts(&unfocused).contains(&"6".to_owned()),
        "below the box"
    );
}

#[test]
fn wrapped_text_ends_with_an_ellipsis_when_it_overflows() {
    let mut text = TextSystem::bundled_only();
    let long = "some notes that go on and on, much longer than two lines of a narrow card";
    // The root fills the frame; the card-like box inside it is what's sized.
    let root: Node<Msg> = Node::column().child(
        Node::column()
            .width(Length::Fixed(120.0))
            .height(Length::Fixed(2.0 * TextSystem::line_height(12.0)))
            .child(wrapped_text(long, Font::Ui, 12.0, Theme::dark().text)),
    );
    let frame = build(&root, Rect::new(0.0, 0.0, 400.0, 400.0), &mut text, None);
    let shown = texts(&frame);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(
        long.starts_with(&shown[0]) && shown[1].ends_with('…'),
        "{shown:?}"
    );
    let short: Node<Msg> = Node::column()
        .width(Length::Fixed(400.0))
        .height(Length::Fixed(40.0))
        .child(wrapped_text(
            "short\n\nnote",
            Font::Ui,
            12.0,
            Theme::dark().text,
        ));
    let frame = build(&short, Rect::new(0.0, 0.0, 400.0, 400.0), &mut text, None);
    assert_eq!(texts(&frame), ["short", "note"], "blank lines draw nothing");
}
