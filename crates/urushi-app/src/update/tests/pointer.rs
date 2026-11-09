//! The mouse over terminals: reporting to applications that ask for it, Shift to
//! select anyway, the wheel, and older daemons.

use std::sync::Arc;

use urushi_client::v1::{self, notification};
use urushi_ui::Point;

use super::{attached, bodies, id};
use crate::keys::Mods;
use crate::msg::{Effect, Msg};
use crate::state::{AppState, SettingToggle};

/// A pane whose application turned on mouse reporting, on a 1.3 daemon.
fn reporting() -> AppState {
    let mut s = attached();
    s.daemon_minor = 3;
    Arc::make_mut(s.screens.get_mut(&id(100)).unwrap())
        .modes
        .mouse_reporting = true;
    s
}

fn mouse(fx: &[Effect]) -> Vec<(v1::MouseButton, v1::MouseAction, u32, u32)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Notify(notification::Body::Input(v1::Input {
                kind: Some(v1::input::Kind::Mouse(m)),
                ..
            })) => Some((m.button(), m.action(), m.col, m.row)),
            _ => None,
        })
        .collect()
}

#[test]
fn clicks_and_drags_go_to_apps_that_take_the_mouse() {
    let mut s = reporting();
    let fx = s.update(Msg::AreaPress(Point::new(30.0, 40.0)));
    let press = mouse(&fx);
    assert_eq!(press.len(), 1);
    assert_eq!(
        (press[0].0, press[0].1),
        (v1::MouseButton::Left, v1::MouseAction::Press)
    );
    assert!(
        s.selection.is_none(),
        "no text selection while the app has the mouse"
    );

    let fx = s.update(Msg::AreaDrag(Point::new(70.0, 40.0)));
    assert_eq!(mouse(&fx)[0].1, v1::MouseAction::Move);
    assert!(
        mouse(&s.update(Msg::AreaDrag(Point::new(71.0, 41.0)))).is_empty(),
        "same cell"
    );
    let fx = s.update(Msg::AreaRelease(Point::new(70.0, 40.0)));
    assert_eq!(mouse(&fx)[0].1, v1::MouseAction::Release);
    assert_eq!(press[0].3, mouse(&fx)[0].3, "same row");
}

#[test]
fn shift_selects_text_even_when_the_app_takes_the_mouse() {
    let mut s = reporting();
    s.update(Msg::Modifiers(Mods {
        shift: true,
        ..Mods::default()
    }));
    let fx = s.update(Msg::AreaPress(Point::new(30.0, 40.0)));
    assert!(mouse(&fx).is_empty());
    s.update(Msg::AreaDrag(Point::new(90.0, 40.0)));
    assert!(s.selection.is_some());
}

#[test]
fn hover_and_other_buttons_are_reported_once_per_cell() {
    let mut s = reporting();
    let fx = s.update(Msg::AreaHover(Point::new(30.0, 40.0)));
    assert_eq!(mouse(&fx)[0].0, v1::MouseButton::None);
    assert!(s.update(Msg::AreaHover(Point::new(31.0, 41.0))).is_empty());
    let fx = s.update(Msg::AreaButton {
        at: Point::new(30.0, 40.0),
        button: v1::MouseButton::Right,
        pressed: true,
    });
    assert_eq!(mouse(&fx)[0].0, v1::MouseButton::Right);

    // Without mouse reporting, none of this is sent.
    let mut plain = attached();
    plain.daemon_minor = 3;
    assert!(
        plain
            .update(Msg::AreaHover(Point::new(30.0, 40.0)))
            .is_empty()
    );
    assert!(
        plain
            .update(Msg::AreaButton {
                at: Point::new(30.0, 40.0),
                button: v1::MouseButton::Right,
                pressed: true,
            })
            .is_empty()
    );
}

#[test]
fn the_wheel_is_a_wheel_input_or_scroll_pane_for_older_daemons() {
    let mut s = attached();
    s.daemon_minor = 3;
    let fx = s.update(Msg::AreaWheel(Point::new(30.0, 40.0), -2));
    assert!(matches!(
        &fx[..],
        [Effect::Notify(notification::Body::Input(v1::Input {
            kind: Some(v1::input::Kind::Wheel(v1::WheelInput {
                up: false,
                lines: 2,
                ..
            })),
            ..
        }))]
    ));
    s.daemon_minor = 2;
    let fx = s.update(Msg::AreaWheel(Point::new(30.0, 40.0), 3));
    assert!(matches!(
        &fx[..],
        [Effect::Notify(notification::Body::ScrollPane(
            v1::ScrollPane { lines: 3, .. }
        ))]
    ));
}

#[test]
fn agent_settings_are_inert_on_a_daemon_that_cant_store_them() {
    let mut s = attached();
    assert_eq!(s.daemon_minor, 0);
    assert!(s.daemon_outdated());
    let fx = s.update(Msg::Toggle(SettingToggle::SkipPermissions(
        urushi_core::Agent::Claude,
    )));
    assert!(bodies(&fx).is_empty());
}
