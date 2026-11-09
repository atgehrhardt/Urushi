//! Panes: focus after a pane closes, and the pane context menu (split, copy, paste,
//! close) opened by a right or two-finger click.

use std::sync::Arc;

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Direction, Layout, PaneId};
use urushi_proto::convert;
use urushi_ui::Point;

use super::{attached, bodies, connected, id, named, workspace};
use crate::keys::Named;
use crate::msg::{Effect, Followup, Msg, PaneMsg};
use crate::organize::MenuTarget;
use crate::state::AppState;

/// The workspace with the tab split into pane 100 (left) and pane 101 (right), and
/// `focused` focused.
fn two_panes(focused: u128) -> v1::Workspace {
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
    w.tabs[0].focused_pane_id = id(focused);
    w
}

fn changed(s: &mut AppState, w: v1::Workspace) {
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
}

/// Pane 100 split into 100 and 101; the split's reply focused 101 locally, before the
/// daemon reported it focused.
fn split_once() -> AppState {
    let mut s = connected();
    changed(&mut s, two_panes(100));
    s.update(Msg::Response {
        followup: Followup::PaneCreated,
        result: Ok(Outcome::PaneCreated(v1::PaneCreated {
            tab_id: s.current_tab().unwrap().id.clone(),
            pane_id: id(101),
        })),
    });
    assert_eq!(
        s.focused_pane(),
        Some(id(101)),
        "focused before the daemon says so"
    );
    s
}

fn split_target(s: &mut AppState) -> Option<String> {
    let fx = s.update(Msg::Split(v1::Direction::Right));
    bodies(&fx).iter().find_map(|b| match b {
        Body::SplitPane(r) => Some(r.pane_id.clone()),
        _ => None,
    })
}

#[test]
fn closing_a_pane_hands_focus_to_the_one_that_remains() {
    let mut s = split_once();
    // The daemon closed 101 (Cmd+W): pane 100 is all that is left, and focused.
    let mut w = workspace();
    w.revision = 3;
    changed(&mut s, w);
    assert_eq!(s.focused_pane(), Some(id(100)));
    assert_eq!(split_target(&mut s), Some(id(100)));
}

#[test]
fn a_pane_whose_process_exited_is_not_split_or_closed_again() {
    let mut s = split_once();
    s.update(Msg::Daemon(v1::event::Body::PaneExited(v1::PaneExited {
        pane_id: id(101),
        ..Default::default()
    })));
    let mut w = workspace();
    w.revision = 3;
    changed(&mut s, w);
    assert_eq!(split_target(&mut s), Some(id(100)));
    let fx = s.update(Msg::ClosePane);
    assert!(
        bodies(&fx)
            .iter()
            .all(|b| !matches!(b, Body::ClosePane(r) if r.pane_id == id(101))),
        "{fx:?}"
    );
}

#[test]
fn local_focus_survives_until_the_daemon_catches_up() {
    let mut s = split_once();
    // An unrelated update that still focuses 100: the local choice (101) stands.
    changed(&mut s, two_panes(100));
    assert_eq!(s.focused_pane(), Some(id(101)));
    // The daemon agrees; then it moves focus elsewhere, and the client follows it.
    changed(&mut s, two_panes(101));
    changed(&mut s, two_panes(100));
    assert_eq!(s.focused_pane(), Some(id(100)));
}

#[test]
fn a_stale_local_focus_is_ignored_even_before_the_workspace_is_cleaned() {
    let mut s = attached();
    let tab = s.current_tab().unwrap().id.clone();
    s.focused_override.insert(tab, id(999));
    assert_eq!(s.focused_pane(), Some(id(100)));
}

/// Attached, with the tab split into 100 (left half) and 101 (right half), 100 focused.
fn attached_two() -> AppState {
    let mut s = attached();
    changed(&mut s, two_panes(100));
    s
}

fn right_click(s: &mut AppState, x: f32) -> Vec<Effect> {
    s.update(Msg::AreaButton {
        at: Point::new(x, 40.0),
        button: v1::MouseButton::Right,
        pressed: true,
    })
}

#[test]
fn a_right_click_opens_the_menu_of_the_pane_under_it_and_focuses_it() {
    let mut s = attached_two();
    let fx = right_click(&mut s, 600.0);
    let menu = s.menu.as_ref().expect("menu opens");
    assert_eq!(menu.target, MenuTarget::Pane(id(101)));
    assert_eq!(menu.at, Point::new(600.0, 40.0));
    assert_eq!(s.focused_pane(), Some(id(101)));
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::FocusPane(r) if r.pane_id == id(101))),
        "{fx:?}"
    );
    // Releasing the button does nothing more.
    let fx = s.update(Msg::AreaButton {
        at: Point::new(600.0, 40.0),
        button: v1::MouseButton::Right,
        pressed: false,
    });
    assert!(fx.is_empty());
    assert!(s.menu.is_some());
}

#[test]
fn the_menu_opens_even_over_an_app_that_takes_the_mouse() {
    let mut s = attached();
    s.daemon_minor = 3;
    Arc::make_mut(s.screens.get_mut(&id(100)).unwrap())
        .modes
        .mouse_reporting = true;
    let fx = right_click(&mut s, 30.0);
    assert_eq!(
        s.menu.as_ref().map(|m| &m.target),
        Some(&MenuTarget::Pane(id(100)))
    );
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::Notify(v1::notification::Body::Input(_)))),
        "the right click is not sent to the app: {fx:?}"
    );
}

#[test]
fn the_menu_splits_and_closes_its_own_pane() {
    let mut s = attached_two();
    right_click(&mut s, 600.0);
    // Focus moves back to the left pane while the menu is open: the menu still acts
    // on the pane it was opened on.
    s.focused_override.clear();
    let fx = s.update(Msg::Pane(PaneMsg::Split {
        pane: id(101),
        side: v1::Direction::Down,
    }));
    assert!(s.menu.is_none(), "an action closes the menu");
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::SplitPane(r)] if r.pane_id == id(101) && r.side() == v1::Direction::Down
        ),
        "{fx:?}"
    );

    right_click(&mut s, 600.0);
    let fx = s.update(Msg::Pane(PaneMsg::Close(id(101))));
    assert!(s.menu.is_none());
    assert!(
        matches!(bodies(&fx)[..], [Body::ClosePane(r)] if r.pane_id == id(101)),
        "{fx:?}"
    );
    // A pane that has gone meanwhile is left alone.
    changed(&mut s, workspace());
    assert!(bodies(&s.update(Msg::Pane(PaneMsg::Close(id(101))))).is_empty());
}

#[test]
fn the_menu_pastes_and_closes_on_escape() {
    let mut s = attached_two();
    right_click(&mut s, 30.0);
    let fx = s.update(Msg::Pane(PaneMsg::Paste));
    assert!(matches!(fx[..], [Effect::RequestPaste]), "{fx:?}");
    assert!(s.menu.is_none());

    right_click(&mut s, 30.0);
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.menu.is_none());
}
