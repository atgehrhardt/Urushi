//! Dragging a divider (UR-68, ADR 0055): the panes follow the pointer at once, one move
//! is sent at a time, and the preview gives way to the daemon's layout.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Direction, Layout, PaneId};
use urushi_proto::convert;
use urushi_ui::Point;

use super::{attached, bodies, id, workspace};
use crate::msg::{Followup, Msg};
use crate::state::AppState;

fn layout(position: Option<f64>) -> Layout {
    let (a, b) = (PaneId::from_u128(100), PaneId::from_u128(101));
    let mut layout = Layout::single(a);
    layout.split(a, b, Direction::Right).unwrap();
    if let Some(position) = position {
        layout
            .move_divider(a, urushi_core::Axis::Row, position)
            .unwrap();
    }
    layout
}

fn changed(s: &mut AppState, position: Option<f64>) {
    let mut w = workspace();
    w.revision += 1;
    w.tabs[0].layout = Some(convert::layout_to_proto(&layout(position)));
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
}

/// How wide the left pane is on screen.
fn left_width(s: &AppState) -> f32 {
    s.pane_rects()
        .into_iter()
        .find(|(p, _)| *p == id(100))
        .unwrap()
        .1
        .width
}

fn moves(fx: &[crate::msg::Effect]) -> Vec<f64> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::MoveDivider(r) => Some(r.position),
            _ => None,
        })
        .collect()
}

fn answer(s: &mut AppState) -> Vec<crate::msg::Effect> {
    s.update(Msg::Response {
        followup: Followup::DividerMoved,
        result: Ok(Outcome::Ok(v1::Empty {})),
    })
}

#[test]
fn the_divider_follows_the_pointer_before_the_daemon_answers() {
    let mut s = attached();
    changed(&mut s, None);
    let half = left_width(&s);
    let x = s.pane_rects()[0].1.right();
    s.update(Msg::AreaPress(Point::new(x + 1.0, 100.0)));
    let fx = s.update(Msg::AreaDrag(Point::new(200.0, 100.0)));
    assert_eq!(moves(&fx).len(), 1, "{fx:?}");
    let dragged = left_width(&s);
    assert!(dragged < half - 100.0, "{dragged} vs {half}");

    // While that move is unanswered, more moves only update the screen.
    assert!(moves(&s.update(Msg::AreaDrag(Point::new(250.0, 100.0)))).is_empty());
    assert!(moves(&s.update(Msg::AreaDrag(Point::new(300.0, 100.0)))).is_empty());
    assert!(left_width(&s) > dragged + 50.0);
    // The answer sends only the newest position.
    let next = moves(&answer(&mut s));
    assert_eq!(next.len(), 1);
    s.update(Msg::AreaRelease(Point::new(300.0, 100.0)));
    assert!(moves(&answer(&mut s)).is_empty());
    let shown = left_width(&s);

    // The daemon's workspace from before the drop doesn't pull the divider back.
    changed(&mut s, None);
    assert!((left_width(&s) - shown).abs() < 1.0);
    // Once it shows the drop, the preview has done its job.
    changed(&mut s, Some(next[0]));
    assert!(s.divider.group().is_none());
    assert!((left_width(&s) - shown).abs() < 1.0);
}
