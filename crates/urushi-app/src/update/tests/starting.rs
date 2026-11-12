//! Tabs and splits shown before the daemon has made them (UR-68, ADR 0056).

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{Direction, Layout, PaneId};
use urushi_proto::convert;

use super::{attached, bodies, id, key, named, shortcut, workspace};
use crate::keys::Named;
use crate::msg::{Followup, Msg};

fn thread() -> String {
    workspace().threads[0].id.clone()
}

#[test]
fn a_split_shows_at_once_and_a_second_waits_for_it() {
    let mut s = attached();
    let whole = s.pane_rects()[0].1.width;
    let fx = s.update(Msg::Split(v1::Direction::Right));
    assert!(bodies(&fx).iter().any(|b| matches!(b, Body::SplitPane(_))));
    let rects = s.pane_rects();
    assert_eq!(rects.len(), 1, "the placeholder isn't a pane: {rects:?}");
    assert!(rects[0].1.width < whole * 0.6, "the pane made room at once");
    let again = s.update(Msg::Split(v1::Direction::Right));
    assert!(
        !bodies(&again)
            .iter()
            .any(|b| matches!(b, Body::SplitPane(_))),
        "no second split while the first is on its way"
    );

    // The daemon's answer, then its workspace with the real pane.
    let tab = s.current_tab().unwrap().id.clone();
    s.update(Msg::Response {
        followup: Followup::PaneCreated,
        result: Ok(Outcome::PaneCreated(v1::PaneCreated {
            tab_id: tab,
            pane_id: id(101),
        })),
    });
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
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    assert!(s.starting.split_group().is_none());
    assert_eq!(s.pane_rects().len(), 2);
}

#[test]
fn a_refused_split_puts_the_pane_back() {
    let mut s = attached();
    let whole = s.pane_rects()[0].1.width;
    s.update(Msg::Split(v1::Direction::Right));
    s.update(Msg::Response {
        followup: Followup::PaneCreated,
        result: Err("can't start a shell".into()),
    });
    assert!(s.starting.split_group().is_none());
    assert!((s.pane_rects()[0].1.width - whole).abs() < 1.0);
    assert_eq!(s.error.as_deref(), Some("can't start a shell"));
}

#[test]
fn a_new_tab_shows_as_starting_until_the_daemon_answers() {
    let mut s = attached();
    s.update(Msg::Key(key("t", shortcut())));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    let followup = fx.iter().find_map(|e| match e {
        crate::msg::Effect::Request {
            body: Body::CreateTab(_),
            followup,
        } => Some(followup.clone()),
        _ => None,
    });
    let Some(Followup::TabCreated(k)) = followup else {
        panic!("{fx:?}");
    };
    assert_eq!(s.starting.tabs_of(&thread()).count(), 1);
    s.update(Msg::Response {
        followup: Followup::TabCreated(k),
        result: Err("agent not installed".into()),
    });
    assert_eq!(s.starting.tabs_of(&thread()).count(), 0);
    assert_eq!(s.error.as_deref(), Some("agent not installed"));
}
