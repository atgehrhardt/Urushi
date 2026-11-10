//! Hiding, showing and resizing the desktop sidebar.

use urushi_ui::Point;

use super::{connected, key, shortcut};
use crate::msg::{Msg, SidebarMsg};
use crate::sidebar::{DEFAULT_WIDTH, MAX_WIDTH, MIN_WIDTH};
use crate::state::AppState;
use crate::view::PANEL_MARGIN;

/// Presses the sidebar's edge at `x` and drags it, as the runner does.
fn grab_edge(s: &mut AppState, x: f32) {
    s.update(Msg::Pointer(Point::new(x, 300.0)));
    s.update(Msg::Sidebar(SidebarMsg::EdgePressed));
    s.update(Msg::Sidebar(SidebarMsg::StartResize));
}

fn move_to(s: &mut AppState, x: f32) {
    s.update(Msg::DragMove(Point::new(x, 300.0)));
}

#[test]
fn the_button_and_the_shortcut_hide_and_show_it() {
    let mut s = connected();
    s.update(Msg::Sidebar(SidebarMsg::Toggle));
    assert!(s.sidebar.collapsed());
    s.update(Msg::Key(key("\\", shortcut())));
    assert!(!s.sidebar.collapsed());
    // With Shift held, the key reads as "|".
    s.update(Msg::Key(key("|", shortcut())));
    assert!(s.sidebar.collapsed());
}

#[test]
fn phones_ignore_it() {
    let mut s = connected();
    s.update(Msg::SetCompact(true));
    s.update(Msg::Sidebar(SidebarMsg::Toggle));
    assert!(!s.sidebar.collapsed());
    grab_edge(&mut s, 300.0);
    assert!(!s.sidebar.resizing());
}

#[test]
fn dragging_the_edge_resizes_it_within_limits() {
    let mut s = connected();
    s.update(Msg::Sidebar(SidebarMsg::WindowWidth(1600.0)));
    let edge = PANEL_MARGIN + DEFAULT_WIDTH;
    grab_edge(&mut s, edge + 2.0);
    assert!(s.sidebar.resizing());
    move_to(&mut s, edge + 2.0 + 60.0);
    assert!((s.sidebar.width() - (DEFAULT_WIDTH + 60.0)).abs() < 0.01);
    move_to(&mut s, 5000.0);
    assert!((s.sidebar.width() - MAX_WIDTH).abs() < 0.01);
    move_to(&mut s, 200.0);
    assert!((s.sidebar.width() - MIN_WIDTH).abs() < 0.01);
    assert!(s.dragging.is_none(), "no ghost or drop targets");

    // Letting go over nothing sends DragCancel, which ends the resize only.
    s.update(Msg::DragCancel);
    assert!(!s.sidebar.resizing());
    assert!((s.sidebar.width() - MIN_WIDTH).abs() < 0.01);
}

#[test]
fn dragging_it_nearly_shut_collapses_it_and_keeps_the_width() {
    let mut s = connected();
    let edge = PANEL_MARGIN + DEFAULT_WIDTH;
    grab_edge(&mut s, edge);
    move_to(&mut s, PANEL_MARGIN + 320.0);
    move_to(&mut s, PANEL_MARGIN + 40.0);
    assert!(s.sidebar.collapsed());
    s.update(Msg::DragCancel);
    s.update(Msg::Sidebar(SidebarMsg::Toggle));
    assert!(!s.sidebar.collapsed());
    assert!(
        (s.sidebar.width() - 320.0).abs() < 0.01,
        "reopens as wide as before"
    );
}

#[test]
fn other_drags_are_untouched() {
    let mut s = connected();
    // No edge grabbed: DragMove and DragCancel go where they always did.
    move_to(&mut s, 400.0);
    s.update(Msg::DragCancel);
    assert!((s.sidebar.width() - DEFAULT_WIDTH).abs() < 0.01);
}

#[test]
fn the_slide_follows_the_runner() {
    let mut s = connected();
    s.update(Msg::Sidebar(SidebarMsg::Toggle));
    assert!(s.sidebar.visible(), "still sliding");
    assert!((s.sidebar.reveal_target() - 0.0).abs() < f32::EPSILON);
    s.update(Msg::Sidebar(SidebarMsg::Reveal(0.0)));
    assert!(!s.sidebar.visible());
}
