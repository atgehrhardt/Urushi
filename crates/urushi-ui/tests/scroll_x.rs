//! Sideways scrolling rows, and the minimum width of a `Fill` child in a row.
#![allow(clippy::unwrap_used, clippy::float_cmp, reason = "test code")]

use urushi_ui::Axis;
use urushi_ui::{Color, Length, Node, Point, Primitive, Rect, TextSystem, build, key};

fn quads(scene: &urushi_ui::Scene) -> Vec<Rect> {
    scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            Primitive::Quad(q) => Some(q.rect),
            _ => None,
        })
        .collect()
}

/// `n` equal columns of at least `min` px, in a sideways scrolling row of `width`.
fn columns(n: usize, min: f32, width: f32, offset: f32) -> Node<()> {
    Node::row()
        .width(Length::Fixed(width))
        .height(Length::Fixed(50.0))
        .gap(10.0)
        .scroll_x(key(&["cols"]), offset)
        .children((0..n).map(|_| {
            Node::new()
                .width(Length::Fill(1.0))
                .min_width(min)
                .height(Length::Fill(1.0))
                .background(Color::hex(0x00_00ff))
        }))
}

#[test]
fn fill_children_share_the_row_when_they_fit() {
    let mut text = TextSystem::bundled_only();
    let frame = build(
        &columns(2, 100.0, 410.0, 0.0),
        Rect::new(0.0, 0.0, 410.0, 50.0),
        &mut text,
        None,
    );
    let q = quads(&frame.scene);
    assert_eq!((q[0].width, q[1].x), (200.0, 210.0), "{q:?}");
    let region = frame.scroll_regions[0];
    assert_eq!(region.axis, Axis::Horizontal);
    assert_eq!(region.max_offset(), 0.0, "nothing to scroll");
}

#[test]
fn fill_children_keep_their_minimum_and_the_row_scrolls_sideways() {
    let mut text = TextSystem::bundled_only();
    // Four columns of 100 and three gaps of 10 need 430; 250 show.
    let frame = build(
        &columns(4, 100.0, 250.0, 1000.0),
        Rect::new(0.0, 0.0, 250.0, 50.0),
        &mut text,
        None,
    );
    let region = frame
        .scroll_region_along(Point::new(10.0, 10.0), Axis::Horizontal)
        .unwrap();
    assert_eq!(region.content_len, 430.0);
    assert_eq!(region.offset, 180.0, "clamped to content - viewport");
    let q = quads(&frame.scene);
    assert!(q.iter().all(|r| r.width == 100.0), "{q:?}");
    // The first column, scrolled out of sight, isn't painted; the second is cut.
    assert_eq!(q[0].x, -70.0);
    assert!(frame.scene.dump().starts_with("clip 0,0 250x50"));
    assert!(
        frame
            .scroll_region_along(Point::new(10.0, 10.0), Axis::Vertical)
            .is_none()
    );
}

#[test]
fn a_column_scrolling_inside_a_sideways_row_is_found_by_its_axis() {
    let mut text = TextSystem::bundled_only();
    let column = Node::column()
        .width(Length::Fill(1.0))
        .min_width(200.0)
        .height(Length::Fill(1.0))
        .scroll(key(&["col"]), 0.0)
        .children((0..10).map(|_| Node::new().height(Length::Fixed(20.0))));
    let ui: Node<()> = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .scroll_x(key(&["row"]), 0.0)
        .child(column.clone())
        .child(column);
    let frame = build(&ui, Rect::new(0.0, 0.0, 300.0, 100.0), &mut text, None);
    let p = Point::new(10.0, 10.0);
    assert_eq!(
        frame.scroll_region_along(p, Axis::Vertical).unwrap().key,
        key(&["col"])
    );
    let row = frame.scroll_region_along(p, Axis::Horizontal).unwrap();
    assert_eq!((row.key, row.content_len), (key(&["row"]), 400.0));
}
