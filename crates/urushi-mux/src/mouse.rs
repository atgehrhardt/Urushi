//! Turns abstract mouse events into the reports a terminal application asked for
//! (xterm conventions), and decides what the wheel does.
//!
//! Like key encoding, this lives in the daemon because it depends on modes only the
//! emulator knows: which events the application wants (1000/1002/1003), how to encode
//! them (SGR 1006, UTF-8 1005, or legacy X10), and whether the wheel should become arrow
//! keys on the alternate screen (1007).

use crate::keys::{Key, KeyEvent, Modifiers, encode_key};
use crate::screen::{Modes, MouseEncoding, MouseMode};

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary.
    Left,
    /// Wheel click.
    Middle,
    /// Secondary.
    Right,
    /// No button (movement only).
    None,
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    /// A button went down.
    Press,
    /// A button came up.
    Release,
    /// The pointer moved (with `button` held, or `None`).
    Move,
}

/// A mouse event over a pane, in cells from the top-left (0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    /// Which button.
    pub button: MouseButton,
    /// What happened.
    pub action: MouseAction,
    /// Column.
    pub col: u16,
    /// Row.
    pub row: u16,
    /// Modifiers held.
    pub modifiers: Modifiers,
}

/// Wheel movement over a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelEvent {
    /// Towards older content (wheel up / two fingers down with natural scrolling).
    pub up: bool,
    /// How many lines (notches).
    pub lines: u16,
    /// Column under the pointer.
    pub col: u16,
    /// Row under the pointer.
    pub row: u16,
    /// Modifiers held.
    pub modifiers: Modifiers,
}

/// What the wheel does in the current modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WheelAction {
    /// Send these bytes to the application (mouse reports or arrow keys).
    Send(Vec<u8>),
    /// Scroll the terminal's own scrollback by this many lines (positive = back).
    Scrollback(i32),
}

/// The report for `event`, or nothing if the application didn't ask for it.
pub fn encode_mouse(event: &MouseEvent, modes: Modes) -> Vec<u8> {
    // Presses and releases need a button; so does movement in button-event tracking
    // (1002). Any-motion tracking (1003) reports all movement.
    let wanted = match (modes.mouse, event.action) {
        (MouseMode::Off, _) | (MouseMode::Click, MouseAction::Move) => false,
        (MouseMode::Motion, MouseAction::Move) => true,
        (_, MouseAction::Press | MouseAction::Release) | (MouseMode::Drag, MouseAction::Move) => {
            event.button != MouseButton::None
        }
    };
    if !wanted {
        return Vec::new();
    }
    let mut code = match event.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
    };
    if event.action == MouseAction::Move {
        code += 32;
    }
    let release = event.action == MouseAction::Release;
    // Legacy encodings can't say which button was released.
    if release && modes.mouse_encoding != MouseEncoding::Sgr {
        code = 3;
    }
    report(
        code + modifier_bits(event.modifiers),
        event.col,
        event.row,
        release,
        modes,
    )
}

/// What a wheel movement does: mouse reports if the application wants the mouse,
/// arrow keys on the alternate screen (1007), otherwise scrollback.
pub fn wheel(event: &WheelEvent, modes: Modes) -> WheelAction {
    if modes.mouse != MouseMode::Off {
        let code = if event.up { 64 } else { 65 } + modifier_bits(event.modifiers);
        let one = report(code, event.col, event.row, false, modes);
        return WheelAction::Send(one.repeat(usize::from(event.lines)));
    }
    if modes.alt_screen && modes.alternate_scroll {
        let key = KeyEvent {
            key: if event.up { Key::Up } else { Key::Down },
            modifiers: Modifiers::default(),
        };
        return WheelAction::Send(encode_key(&key, modes).repeat(usize::from(event.lines)));
    }
    let lines = i32::from(event.lines);
    WheelAction::Scrollback(if event.up { lines } else { -lines })
}

fn modifier_bits(m: Modifiers) -> u8 {
    u8::from(m.shift) * 4 + u8::from(m.alt) * 8 + u8::from(m.ctrl) * 16
}

fn report(code: u8, col: u16, row: u16, release: bool, modes: Modes) -> Vec<u8> {
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    match modes.mouse_encoding {
        MouseEncoding::Sgr => {
            format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        }
        MouseEncoding::Utf8 => {
            let mut out = b"\x1b[M".to_vec();
            for v in [u32::from(code), x, y] {
                let Some(c) = char::from_u32(v + 32).filter(|_| v + 32 <= 2047) else {
                    return Vec::new();
                };
                let mut buf = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            out
        }
        MouseEncoding::X10 => {
            let mut out = b"\x1b[M".to_vec();
            for v in [u32::from(code), x, y] {
                match u8::try_from(v + 32) {
                    Ok(b) => out.push(b),
                    // Too far right or down for this encoding: report nothing.
                    Err(_) => return Vec::new(),
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(mouse: MouseMode, mouse_encoding: MouseEncoding) -> Modes {
        Modes {
            mouse,
            mouse_encoding,
            ..Modes::default()
        }
    }

    fn event(button: MouseButton, action: MouseAction) -> MouseEvent {
        MouseEvent {
            button,
            action,
            col: 9,
            row: 4,
            modifiers: Modifiers::default(),
        }
    }

    fn wheel_at(up: bool, lines: u16) -> WheelEvent {
        WheelEvent {
            up,
            lines,
            col: 0,
            row: 0,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn sgr_reports_press_release_and_drag() {
        let m = modes(MouseMode::Drag, MouseEncoding::Sgr);
        assert_eq!(
            encode_mouse(&event(MouseButton::Left, MouseAction::Press), m),
            b"\x1b[<0;10;5M"
        );
        assert_eq!(
            encode_mouse(&event(MouseButton::Left, MouseAction::Move), m),
            b"\x1b[<32;10;5M"
        );
        assert_eq!(
            encode_mouse(&event(MouseButton::Left, MouseAction::Release), m),
            b"\x1b[<0;10;5m"
        );
        assert_eq!(
            encode_mouse(&event(MouseButton::Right, MouseAction::Press), m),
            b"\x1b[<2;10;5M"
        );
        // Plain movement is only reported in any-motion mode.
        assert!(encode_mouse(&event(MouseButton::None, MouseAction::Move), m).is_empty());
        let any = modes(MouseMode::Motion, MouseEncoding::Sgr);
        assert_eq!(
            encode_mouse(&event(MouseButton::None, MouseAction::Move), any),
            b"\x1b[<35;10;5M"
        );
    }

    #[test]
    fn click_mode_ignores_motion_and_off_reports_nothing() {
        let click = modes(MouseMode::Click, MouseEncoding::Sgr);
        assert!(encode_mouse(&event(MouseButton::Left, MouseAction::Move), click).is_empty());
        let off = Modes::default();
        assert!(encode_mouse(&event(MouseButton::Left, MouseAction::Press), off).is_empty());
    }

    #[test]
    fn modifiers_and_legacy_encodings() {
        let mut e = event(MouseButton::Left, MouseAction::Press);
        e.modifiers.ctrl = true;
        e.modifiers.shift = true;
        let x10 = modes(MouseMode::Click, MouseEncoding::X10);
        assert_eq!(
            encode_mouse(&e, x10),
            [0x1b, b'[', b'M', 32 + 20, 32 + 10, 32 + 5]
        );
        // Legacy releases don't name the button.
        let release = encode_mouse(&event(MouseButton::Left, MouseAction::Release), x10);
        assert_eq!(release, [0x1b, b'[', b'M', 32 + 3, 32 + 10, 32 + 5]);
        // Beyond column 223, X10 can't report; UTF-8 can.
        let mut far = event(MouseButton::Left, MouseAction::Press);
        far.col = 300;
        assert!(encode_mouse(&far, x10).is_empty());
        let utf8 = encode_mouse(&far, modes(MouseMode::Click, MouseEncoding::Utf8));
        assert_eq!(String::from_utf8(utf8).unwrap(), "\x1b[M \u{14d}%");
    }

    #[test]
    fn the_wheel_reports_scrolls_or_sends_arrows() {
        let sgr = modes(MouseMode::Click, MouseEncoding::Sgr);
        assert_eq!(
            wheel(&wheel_at(true, 2), sgr),
            WheelAction::Send(b"\x1b[<64;1;1M\x1b[<64;1;1M".to_vec())
        );
        assert_eq!(
            wheel(&wheel_at(false, 1), sgr),
            WheelAction::Send(b"\x1b[<65;1;1M".to_vec())
        );

        let alt = Modes {
            alt_screen: true,
            alternate_scroll: true,
            ..Modes::default()
        };
        assert_eq!(
            wheel(&wheel_at(true, 3), alt),
            WheelAction::Send(b"\x1b[A\x1b[A\x1b[A".to_vec())
        );
        let app_cursor = Modes {
            app_cursor: true,
            ..alt
        };
        assert_eq!(
            wheel(&wheel_at(false, 1), app_cursor),
            WheelAction::Send(b"\x1bOB".to_vec())
        );

        assert_eq!(
            wheel(&wheel_at(true, 3), Modes::default()),
            WheelAction::Scrollback(3)
        );
        assert_eq!(
            wheel(&wheel_at(false, 2), Modes::default()),
            WheelAction::Scrollback(-2)
        );
    }
}
