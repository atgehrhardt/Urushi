//! Vertical sliding (`slide_y`), tested through the scene and hit regions (no GPU).
#![allow(clippy::unwrap_used, clippy::float_cmp, reason = "test code")]

use urushi_ui::{Color, Length, Node, Point, Primitive, Rect, TextSystem, build};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Under,
    Panel,
}

fn first_quad(scene: &urushi_ui::Scene) -> Rect {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            Primitive::Quad(q) => Some(q.rect),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_dropping_panel_moves_up_with_its_children_and_is_clipped() {
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
                    .on_click(Msg::Under),
            )
            .child(
                Node::new()
                    .width(Length::Fill(1.0))
                    .height(Length::Fill(1.0))
                    .background(Color::rgb8(1, 2, 3))
                    .on_click(Msg::Panel)
                    .slide_y(slide),
            )
    };
    let bounds = Rect::new(0.0, 100.0, 200.0, 100.0);
    // Half up: the panel moves up by half its height, and only its visible part (the
    // top half of the bounds, inside the clip) takes clicks.
    let frame = build(&ui(0.5), bounds, &mut text, None);
    assert_eq!(first_quad(&frame.scene), Rect::new(0.0, 50.0, 200.0, 100.0));
    assert_eq!(frame.click(Point::new(50.0, 120.0)), Some(Msg::Panel));
    assert_eq!(frame.click(Point::new(50.0, 170.0)), Some(Msg::Under));
    assert_eq!(
        frame.click(Point::new(50.0, 80.0)),
        None,
        "outside the clip"
    );
    // In place and fully up.
    let frame = build(&ui(0.0), bounds, &mut text, None);
    assert_eq!(frame.click(Point::new(50.0, 150.0)), Some(Msg::Panel));
    let frame = build(&ui(1.0), bounds, &mut text, None);
    assert_eq!(frame.click(Point::new(50.0, 105.0)), Some(Msg::Under));
}
