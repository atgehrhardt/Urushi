//! The on-screen keyboard's policy (`soft_keyboard.rs`): text fields open it by
//! themselves, terminals once tapped, and it closes when nothing can take the typing.

use super::{CELL, connected};
use crate::config::ConnectTarget;
use crate::msg::Msg;
use crate::soft_keyboard::SoftKeyboard;
use crate::state::{AppState, Dialog, Field};

/// Connected on a phone, with a thread's terminals on screen.
fn phone_in_thread() -> AppState {
    let mut s = connected();
    s.compact = true;
    s.sidebar_open = false;
    s
}

#[test]
fn the_connect_dialog_opens_the_keyboard_at_launch() {
    let s = AppState::new(ConnectTarget::Ask, CELL);
    let mut keyboard = SoftKeyboard::default();
    assert_eq!(keyboard.sync(&s, false), Some(true));
    assert_eq!(
        keyboard.sync(&s, false),
        None,
        "asked once, not every event"
    );
}

#[test]
fn tapping_the_focused_field_again_brings_a_dismissed_keyboard_back() {
    let mut s = AppState::new(ConnectTarget::Ask, CELL);
    let mut keyboard = SoftKeyboard::default();
    keyboard.sync(&s, false);
    keyboard.tapped(false);
    assert_eq!(keyboard.sync(&s, true), Some(true));
    s.update(Msg::FocusField(Field::PairingCode));
    assert_eq!(keyboard.sync(&s, false), None, "already up");
}

#[test]
fn closing_the_dialog_closes_the_keyboard() {
    let mut s = AppState::new(ConnectTarget::Ask, CELL);
    let mut keyboard = SoftKeyboard::default();
    keyboard.sync(&s, false);
    s.dialog = None;
    s.focus = None;
    assert_eq!(keyboard.sync(&s, false), Some(false));
}

#[test]
fn a_terminal_opens_the_keyboard_only_once_tapped() {
    let s = phone_in_thread();
    let mut keyboard = SoftKeyboard::default();
    assert_eq!(keyboard.sync(&s, false), None);
    keyboard.tapped(false);
    assert_eq!(keyboard.sync(&s, true), None, "a tap off the terminal");
    keyboard.tapped(true);
    assert_eq!(keyboard.sync(&s, true), Some(true));
    keyboard.tapped(true);
    assert_eq!(keyboard.sync(&s, true), Some(true), "tapping again re-asks");
}

#[test]
fn the_thread_list_closes_the_keyboard_and_forgets_the_tap() {
    let mut s = phone_in_thread();
    let mut keyboard = SoftKeyboard::default();
    keyboard.tapped(true);
    keyboard.sync(&s, true);
    s.sidebar_open = true;
    assert_eq!(keyboard.sync(&s, false), Some(false));
    s.sidebar_open = false;
    assert_eq!(keyboard.sync(&s, false), None, "needs a new tap");
}

#[test]
fn a_dialog_without_a_text_field_closes_the_keyboard() {
    let mut s = phone_in_thread();
    let mut keyboard = SoftKeyboard::default();
    keyboard.tapped(true);
    keyboard.sync(&s, true);
    s.dialog = Some(Dialog::connect());
    s.focus = None;
    assert_eq!(keyboard.sync(&s, false), Some(false));
    s.dialog = None;
    assert_eq!(keyboard.sync(&s, false), None);
}

#[test]
fn a_text_field_over_a_tapped_terminal_keeps_the_keyboard_up() {
    let mut s = phone_in_thread();
    let mut keyboard = SoftKeyboard::default();
    keyboard.tapped(true);
    keyboard.sync(&s, true);
    s.dialog = Some(Dialog::connect());
    s.focus = Some(Field::Host);
    assert_eq!(keyboard.sync(&s, false), None);
}
