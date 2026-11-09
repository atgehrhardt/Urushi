//! Conversions between the multiplexer's screen and input types and the wire.

use urushi_core::PaneId;
use urushi_mux::screen::{Color, CursorShape, ScreenFrame};
use urushi_mux::{
    Input, Key, KeyEvent, Modifiers, MouseAction, MouseButton, MouseEvent, WheelEvent,
};
use urushi_proto::convert;
use urushi_proto::v1;

/// A frame → `ScreenUpdate` with sequence number `seq`.
pub(crate) fn to_proto(pane: PaneId, seq: u64, frame: &ScreenFrame) -> v1::ScreenUpdate {
    v1::ScreenUpdate {
        pane_id: pane.to_string(),
        seq,
        full: frame.full,
        cols: u32::from(frame.size.cols),
        rows: u32::from(frame.size.rows),
        lines: frame
            .lines
            .iter()
            .map(|(index, line)| v1::ScreenLine {
                index: u32::from(*index),
                spans: line
                    .spans
                    .iter()
                    .map(|s| v1::Span {
                        text: s.text.clone(),
                        style: Some(v1::Style {
                            fg: color(s.style.fg),
                            bg: color(s.style.bg),
                            flags: s.style.flags,
                        }),
                    })
                    .collect(),
            })
            .collect(),
        cursor: Some(v1::Cursor {
            line: u32::from(frame.cursor.line),
            col: u32::from(frame.cursor.col),
            visible: frame.cursor.visible,
            shape: match frame.cursor.shape {
                CursorShape::Block => v1::CursorShape::Block,
                CursorShape::Underline => v1::CursorShape::Underline,
                CursorShape::Bar => v1::CursorShape::Bar,
            }
            .into(),
        }),
        modes: Some(v1::TerminalModes {
            app_cursor: frame.modes.app_cursor,
            bracketed_paste: frame.modes.bracketed_paste,
            mouse_reporting: frame.modes.mouse_reporting(),
            alt_screen: frame.modes.alt_screen,
        }),
        title: frame.title.clone(),
        display_offset: frame.display_offset,
        history_size: frame.history_size,
    }
}

fn color(c: Color) -> Option<v1::Color> {
    match c {
        Color::Default => None,
        Color::Indexed(i) => Some(v1::Color {
            kind: Some(v1::color::Kind::Indexed(u32::from(i))),
        }),
        Color::Rgb(r, g, b) => Some(v1::Color {
            kind: Some(v1::color::Kind::Rgb(
                u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
            )),
        }),
    }
}

/// Wire input → `(pane, input)`. `None` for malformed or empty input.
pub(crate) fn input_from_proto(input: v1::Input) -> Option<(PaneId, Input)> {
    let pane = convert::parse_id::<PaneId>(&input.pane_id, "pane_id").ok()?;
    let converted = match input.kind? {
        v1::input::Kind::Text(text) => Input::Text(text),
        v1::input::Kind::Paste(text) => Input::Paste(text),
        v1::input::Kind::Key(key) => Input::Key(KeyEvent {
            key: key_from_proto(&key)?,
            modifiers: Modifiers::from_bits(key.modifiers),
        }),
        v1::input::Kind::Mouse(m) => Input::Mouse(mouse_from_proto(&m)?),
        v1::input::Kind::Wheel(w) => Input::Wheel(WheelEvent {
            up: w.up,
            lines: u16::try_from(w.lines.clamp(1, MAX_WHEEL_LINES)).ok()?,
            col: cell(w.col),
            row: cell(w.row),
            modifiers: Modifiers::from_bits(w.modifiers),
        }),
    };
    Some((pane, converted))
}

/// More lines per wheel event than this is a client bug, not a fast scroll.
const MAX_WHEEL_LINES: u32 = 100;

fn cell(v: u32) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
}

fn mouse_from_proto(m: &v1::MouseInput) -> Option<MouseEvent> {
    Some(MouseEvent {
        button: match v1::MouseButton::try_from(m.button).ok()? {
            v1::MouseButton::Left => MouseButton::Left,
            v1::MouseButton::Middle => MouseButton::Middle,
            v1::MouseButton::Right => MouseButton::Right,
            v1::MouseButton::None => MouseButton::None,
            v1::MouseButton::Unspecified => return None,
        },
        action: match v1::MouseAction::try_from(m.action).ok()? {
            v1::MouseAction::Press => MouseAction::Press,
            v1::MouseAction::Release => MouseAction::Release,
            v1::MouseAction::Move => MouseAction::Move,
            v1::MouseAction::Unspecified => return None,
        },
        col: cell(m.col),
        row: cell(m.row),
        modifiers: Modifiers::from_bits(m.modifiers),
    })
}

fn key_from_proto(key: &v1::KeyInput) -> Option<Key> {
    use v1::Key as K;
    Some(match K::try_from(key.key).ok()? {
        K::Character if !key.text.is_empty() => Key::Character(key.text.clone()),
        K::Enter => Key::Enter,
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::Escape => Key::Escape,
        K::Space => Key::Space,
        K::Up => Key::Up,
        K::Down => Key::Down,
        K::Left => Key::Left,
        K::Right => Key::Right,
        K::Home => Key::Home,
        K::End => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        K::Insert => Key::Insert,
        K::Delete => Key::Delete,
        K::F1 => Key::F(1),
        K::F2 => Key::F(2),
        K::F3 => Key::F(3),
        K::F4 => Key::F(4),
        K::F5 => Key::F(5),
        K::F6 => Key::F(6),
        K::F7 => Key::F(7),
        K::F8 => Key::F(8),
        K::F9 => Key::F(9),
        K::F10 => Key::F(10),
        K::F11 => Key::F(11),
        K::F12 => Key::F(12),
        K::Character | K::Unspecified => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_pack_rgb() {
        assert_eq!(
            color(Color::Rgb(1, 2, 3)),
            Some(v1::Color {
                kind: Some(v1::color::Kind::Rgb(0x01_02_03))
            })
        );
        assert_eq!(color(Color::Default), None);
    }

    #[test]
    fn input_conversion() {
        let pane = PaneId::from_u128(1);
        let key = v1::Input {
            pane_id: pane.to_string(),
            kind: Some(v1::input::Kind::Key(v1::KeyInput {
                key: v1::Key::Character.into(),
                text: "c".into(),
                modifiers: 2,
            })),
        };
        let (p, input) = input_from_proto(key).unwrap();
        assert_eq!(p, pane);
        assert_eq!(
            input,
            Input::Key(KeyEvent {
                key: Key::Character("c".into()),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                }
            })
        );
        let empty_char = v1::Input {
            pane_id: pane.to_string(),
            kind: Some(v1::input::Kind::Key(v1::KeyInput {
                key: v1::Key::Character.into(),
                text: String::new(),
                modifiers: 0,
            })),
        };
        assert!(input_from_proto(empty_char).is_none());
        assert!(
            input_from_proto(v1::Input {
                pane_id: "bad".into(),
                kind: None
            })
            .is_none()
        );
    }
}
