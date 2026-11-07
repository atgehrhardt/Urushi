//! Turns abstract key events into the bytes a terminal application expects (xterm
//! conventions).
//!
//! Encoding happens in the daemon, not the client, because it depends on modes that only
//! the emulator knows (e.g. application cursor keys). Super/Cmd chords are for the GUI's
//! own shortcuts and produce no bytes.

use crate::screen::Modes;

/// A key, independent of keyboard layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable key, already shifted by the client's layout (e.g. `"A"` for Shift+a).
    Character(String),
    /// Return.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Space bar.
    Space,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Insert.
    Insert,
    /// Forward delete.
    Delete,
    /// Function keys F1–F12.
    F(u8),
}

/// Modifier keys held during a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors four independent physical keys"
)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Super / Cmd / Windows.
    pub super_key: bool,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        super_key: false,
    };

    /// Builds modifiers from protocol bits (`Modifier` in `terminal.proto`).
    pub const fn from_bits(bits: u32) -> Self {
        Self {
            shift: bits & 1 != 0,
            ctrl: bits & 2 != 0,
            alt: bits & 4 != 0,
            super_key: bits & 8 != 0,
        }
    }

    /// The xterm modifier parameter (`1 + shift + 2·alt + 4·ctrl`), or `None` if none of those are held.
    fn xterm_param(self) -> Option<u8> {
        let value = u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl);
        (value > 0).then_some(value + 1)
    }
}

/// A key press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key.
    pub key: Key,
    /// Held modifiers.
    pub modifiers: Modifiers,
}

const ESC: u8 = 0x1b;

/// Encodes a key press for an application in `modes`. Empty means "send nothing".
pub fn encode_key(event: &KeyEvent, modes: Modes) -> Vec<u8> {
    let m = event.modifiers;
    if m.super_key {
        return Vec::new();
    }
    let alt_prefixed = |mut bytes: Vec<u8>| {
        if m.alt {
            bytes.insert(0, ESC);
        }
        bytes
    };
    match &event.key {
        Key::Character(text) => alt_prefixed(encode_character(text, m.ctrl)),
        Key::Space => alt_prefixed(if m.ctrl { vec![0] } else { vec![b' '] }),
        Key::Enter => alt_prefixed(vec![b'\r']),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt_prefixed(vec![b'\t']),
        Key::Backspace => alt_prefixed(vec![if m.ctrl { 0x08 } else { 0x7f }]),
        Key::Escape => alt_prefixed(vec![ESC]),
        Key::Up => cursor_key(b'A', m, modes),
        Key::Down => cursor_key(b'B', m, modes),
        Key::Right => cursor_key(b'C', m, modes),
        Key::Left => cursor_key(b'D', m, modes),
        Key::Home => cursor_key(b'H', m, modes),
        Key::End => cursor_key(b'F', m, modes),
        Key::Insert => tilde_key(2, m),
        Key::Delete => tilde_key(3, m),
        Key::PageUp => tilde_key(5, m),
        Key::PageDown => tilde_key(6, m),
        Key::F(n @ 1..=4) => {
            let letter = b"PQRS"[usize::from(n - 1)];
            match m.xterm_param() {
                None => vec![ESC, b'O', letter],
                Some(p) => format!("\x1b[1;{p}{}", char::from(letter)).into_bytes(),
            }
        }
        Key::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(CODES[usize::from(n - 5)], m)
        }
        Key::F(_) => Vec::new(),
    }
}

/// Encodes a paste. Newlines become carriage returns, as typed Enter keys would. With
/// bracketed paste the text is wrapped in markers, and any embedded end marker is
/// removed so pasted text can't break out of the bracket.
pub fn encode_paste(text: &str, modes: Modes) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if modes.bracketed_paste {
        let safe = normalized.replace("\x1b[201~", "");
        format!("\x1b[200~{safe}\x1b[201~").into_bytes()
    } else {
        normalized.into_bytes()
    }
}

fn encode_character(text: &str, ctrl: bool) -> Vec<u8> {
    if ctrl {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && let Some(byte) = control_byte(c)
        {
            return vec![byte];
        }
    }
    text.as_bytes().to_vec()
}

/// The C0 control byte for Ctrl+`c`, following xterm.
fn control_byte(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn cursor_key(letter: u8, m: Modifiers, modes: Modes) -> Vec<u8> {
    match m.xterm_param() {
        Some(p) => format!("\x1b[1;{p}{}", char::from(letter)).into_bytes(),
        None if modes.app_cursor => vec![ESC, b'O', letter],
        None => vec![ESC, b'[', letter],
    }
}

fn tilde_key(code: u8, m: Modifiers) -> Vec<u8> {
    match m.xterm_param() {
        Some(p) => format!("\x1b[{code};{p}~").into_bytes(),
        None => format!("\x1b[{code}~").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> Vec<u8> {
        encode_key(&KeyEvent { key, modifiers }, Modes::default())
    }

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };

    #[test]
    fn characters_and_control_chords() {
        assert_eq!(key(Key::Character("a".into()), Modifiers::NONE), b"a");
        assert_eq!(
            key(Key::Character("é".into()), Modifiers::NONE),
            "é".as_bytes()
        );
        assert_eq!(key(Key::Character("c".into()), CTRL), [0x03]);
        assert_eq!(key(Key::Character("C".into()), CTRL), [0x03]);
        assert_eq!(key(Key::Character("[".into()), CTRL), [0x1b]);
        assert_eq!(key(Key::Character("x".into()), ALT), b"\x1bx");
        assert_eq!(key(Key::Space, CTRL), [0]);
        assert!(
            key(
                Key::Character("c".into()),
                Modifiers {
                    super_key: true,
                    ..Modifiers::NONE
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn editing_keys() {
        assert_eq!(key(Key::Enter, Modifiers::NONE), b"\r");
        assert_eq!(key(Key::Backspace, Modifiers::NONE), [0x7f]);
        assert_eq!(key(Key::Backspace, CTRL), [0x08]);
        assert_eq!(key(Key::Tab, SHIFT), b"\x1b[Z");
        assert_eq!(key(Key::Escape, Modifiers::NONE), [0x1b]);
        assert_eq!(key(Key::Delete, Modifiers::NONE), b"\x1b[3~");
        assert_eq!(key(Key::PageUp, CTRL), b"\x1b[5;5~");
    }

    #[test]
    fn cursor_keys_respect_app_cursor_mode() {
        assert_eq!(key(Key::Up, Modifiers::NONE), b"\x1b[A");
        let app = Modes {
            app_cursor: true,
            ..Modes::default()
        };
        assert_eq!(
            encode_key(
                &KeyEvent {
                    key: Key::Up,
                    modifiers: Modifiers::NONE
                },
                app
            ),
            b"\x1bOA"
        );
        assert_eq!(key(Key::Left, CTRL), b"\x1b[1;5D");
        assert_eq!(
            key(
                Key::Right,
                Modifiers {
                    shift: true,
                    alt: true,
                    ..Modifiers::NONE
                }
            ),
            b"\x1b[1;4C"
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(key(Key::F(1), Modifiers::NONE), b"\x1bOP");
        assert_eq!(key(Key::F(2), SHIFT), b"\x1b[1;2Q");
        assert_eq!(key(Key::F(5), Modifiers::NONE), b"\x1b[15~");
        assert_eq!(key(Key::F(12), Modifiers::NONE), b"\x1b[24~");
        assert!(key(Key::F(13), Modifiers::NONE).is_empty());
    }

    #[test]
    fn paste_normalizes_and_brackets() {
        assert_eq!(encode_paste("a\r\nb\nc", Modes::default()), b"a\rb\rc");
        let bracketed = Modes {
            bracketed_paste: true,
            ..Modes::default()
        };
        assert_eq!(
            encode_paste("x\x1b[201~y", bracketed),
            b"\x1b[200~xy\x1b[201~"
        );
    }

    #[test]
    fn modifier_bits() {
        assert_eq!(
            Modifiers::from_bits(2 | 4),
            Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            }
        );
    }
}
