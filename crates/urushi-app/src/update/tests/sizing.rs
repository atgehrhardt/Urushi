//! A pane shown on two clients (desktop and phone) takes the size of the client a
//! person last clicked, tapped or typed in. Nothing else claims it: not hovering, not
//! the wheel, and not frames that arrive while nobody touches this client (UR-56).

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Direction, Layout, PaneId};
use urushi_proto::convert;
use urushi_ui::{Point, Rect};

use super::{attached, bodies, connected, id, key, workspace};
use crate::keys::Mods;
use crate::msg::{Effect, Followup, Msg};
use crate::state::AppState;

/// Another client resized `pane` to 40×20: the daemon sends this client a full frame
/// at that size.
fn resized_elsewhere(s: &mut AppState, pane: u128, seq: u64) {
    let update = v1::ScreenUpdate {
        pane_id: id(pane),
        seq,
        full: true,
        cols: 40,
        rows: 20,
        ..Default::default()
    };
    s.update(Msg::Daemon(v1::event::Body::ScreenUpdate(update)));
}

/// The sizes this client asked for, by pane.
fn resizes(fx: &[Effect]) -> Vec<(String, u32, u32)> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::SetPaneSize(r) => Some((r.pane_id.clone(), r.cols, r.rows)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_frame_from_another_clients_resize_does_not_resize_back_by_itself() {
    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    assert_eq!(s.screens[&id(100)].cols, 40);
    // The window keeps its size: nothing claims the pane back.
    let fx = s.update(Msg::AreaChanged(Rect::new(0.0, 0.0, 816.0, 400.0)));
    assert!(resizes(&fx).is_empty(), "{fx:?}");
}

#[test]
fn a_click_or_tap_in_the_terminal_takes_the_size_back() {
    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::AreaPress(Point::new(30.0, 40.0)));
    assert_eq!(resizes(&fx), [(id(100), 100, 24)]);
    // Once the daemon reports this client's size, clicking again asks nothing.
    s.update(Msg::AreaRelease(Point::new(30.0, 40.0)));
    let update = v1::ScreenUpdate {
        pane_id: id(100),
        seq: 3,
        full: true,
        cols: 100,
        rows: 24,
        ..Default::default()
    };
    s.update(Msg::Daemon(v1::event::Body::ScreenUpdate(update)));
    let fx = s.update(Msg::AreaPress(Point::new(30.0, 40.0)));
    assert!(resizes(&fx).is_empty(), "{fx:?}");
}

#[test]
fn a_right_click_on_the_terminal_takes_the_size_back() {
    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::AreaButton {
        at: Point::new(30.0, 40.0),
        button: v1::MouseButton::Right,
        pressed: true,
    });
    assert_eq!(resizes(&fx), [(id(100), 100, 24)]);
}

#[test]
fn typing_takes_the_size_back() {
    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::Key(key("a", Mods::default())));
    assert_eq!(resizes(&fx), [(id(100), 100, 24)]);

    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::Text("ls".into()));
    assert_eq!(resizes(&fx), [(id(100), 100, 24)]);

    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::Paste("ls".into()));
    assert_eq!(resizes(&fx), [(id(100), 100, 24)]);
}

#[test]
fn hovering_and_the_wheel_leave_the_size_alone() {
    let mut s = attached();
    resized_elsewhere(&mut s, 100, 2);
    let fx = s.update(Msg::AreaHover(Point::new(30.0, 40.0)));
    assert!(resizes(&fx).is_empty(), "{fx:?}");
    let fx = s.update(Msg::AreaWheel(Point::new(30.0, 40.0), 3));
    assert!(resizes(&fx).is_empty(), "{fx:?}");
}

#[test]
fn one_click_takes_back_every_pane_of_the_tab() {
    let mut layout = Layout::single(PaneId::from_u128(100));
    layout
        .split(
            PaneId::from_u128(100),
            PaneId::from_u128(101),
            Direction::Right,
        )
        .unwrap();
    let mut w = workspace();
    w.revision = 2;
    w.tabs[0].layout = Some(convert::layout_to_proto(&layout));
    let mut s = connected();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    let fx = s.update(Msg::AreaChanged(Rect::new(0.0, 0.0, 816.0, 400.0)));
    let mut sizes = Vec::new();
    for body in bodies(&fx) {
        if let Body::AttachPane(r) = body {
            sizes.push((r.pane_id.clone(), r.cols, r.rows));
        }
    }
    assert_eq!(sizes.len(), 2, "{fx:?}");
    for (pane, cols, rows) in &sizes {
        let update = v1::ScreenUpdate {
            pane_id: pane.clone(),
            seq: 1,
            full: true,
            cols: *cols,
            rows: *rows,
            ..Default::default()
        };
        s.update(Msg::Response {
            followup: Followup::Attached(pane.clone()),
            result: Ok(Outcome::Screen(update)),
        });
    }
    resized_elsewhere(&mut s, 100, 2);
    resized_elsewhere(&mut s, 101, 2);
    // A click in the left pane claims both.
    let fx = s.update(Msg::AreaPress(Point::new(30.0, 40.0)));
    let mut claimed = resizes(&fx);
    claimed.sort();
    let mut expected = sizes;
    expected.sort();
    assert_eq!(claimed, expected);
}
