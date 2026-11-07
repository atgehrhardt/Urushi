//! Keyboard input: winit key events → [`KeyPress`] → terminal input or app shortcuts.

use urushi_client::v1;
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Modifier keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent physical keys"
)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Cmd / Windows / Super.
    pub logo: bool,
}

impl From<ModifiersState> for Mods {
    fn from(m: ModifiersState) -> Self {
        Self {
            shift: m.shift_key(),
            ctrl: m.control_key(),
            alt: m.alt_key(),
            logo: m.super_key(),
        }
    }
}

impl Mods {
    /// Whether the platform's app-shortcut modifier is held: Cmd on Apple platforms,
    /// Ctrl+Shift elsewhere (so plain Ctrl+letter still reaches the terminal).
    pub fn is_shortcut(self) -> bool {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            self.logo
        } else {
            self.ctrl && self.shift
        }
    }

    fn bits(self) -> u32 {
        u32::from(self.shift)
            | u32::from(self.ctrl) << 1
            | u32::from(self.alt) << 2
            | u32::from(self.logo) << 3
    }
}

/// A named (non-character) key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    /// Return.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Space.
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
    /// Delete.
    Delete,
    /// F1–F12.
    F(u8),
}

/// A key press, independent of the windowing library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    /// A named key, if it is one.
    pub named: Option<Named>,
    /// The key's character, ignoring modifiers except Shift (e.g. "c" for Ctrl+C).
    pub character: Option<String>,
    /// The text the key produces with all modifiers (layout-aware, e.g. Option+e on macOS).
    pub text: Option<String>,
    /// Held modifiers.
    pub mods: Mods,
}

impl KeyPress {
    /// Converts a winit key press. `None` for releases and keys we don't handle.
    pub fn from_winit(event: &KeyEvent, mods: Mods) -> Option<Self> {
        if !event.state.is_pressed() {
            return None;
        }
        let named = match &event.logical_key {
            Key::Named(n) => match n {
                NamedKey::Enter => Some(Named::Enter),
                NamedKey::Tab => Some(Named::Tab),
                NamedKey::Backspace => Some(Named::Backspace),
                NamedKey::Escape => Some(Named::Escape),
                NamedKey::Space => Some(Named::Space),
                NamedKey::ArrowUp => Some(Named::Up),
                NamedKey::ArrowDown => Some(Named::Down),
                NamedKey::ArrowLeft => Some(Named::Left),
                NamedKey::ArrowRight => Some(Named::Right),
                NamedKey::Home => Some(Named::Home),
                NamedKey::End => Some(Named::End),
                NamedKey::PageUp => Some(Named::PageUp),
                NamedKey::PageDown => Some(Named::PageDown),
                NamedKey::Insert => Some(Named::Insert),
                NamedKey::Delete => Some(Named::Delete),
                NamedKey::F1 => Some(Named::F(1)),
                NamedKey::F2 => Some(Named::F(2)),
                NamedKey::F3 => Some(Named::F(3)),
                NamedKey::F4 => Some(Named::F(4)),
                NamedKey::F5 => Some(Named::F(5)),
                NamedKey::F6 => Some(Named::F(6)),
                NamedKey::F7 => Some(Named::F(7)),
                NamedKey::F8 => Some(Named::F(8)),
                NamedKey::F9 => Some(Named::F(9)),
                NamedKey::F10 => Some(Named::F(10)),
                NamedKey::F11 => Some(Named::F(11)),
                NamedKey::F12 => Some(Named::F(12)),
                _ => return None,
            },
            _ => None,
        };
        // Some platforms (Android) report Enter and Tab as characters.
        let (named, character) = match &event.logical_key {
            Key::Character(c) if c.as_str() == "\n" || c.as_str() == "\r" => {
                (Some(Named::Enter), None)
            }
            Key::Character(c) if c.as_str() == "\t" => (Some(Named::Tab), None),
            Key::Character(c) => (named, Some(c.to_string())),
            _ => (named, None),
        };
        let text = event
            .text
            .as_ref()
            .map(ToString::to_string)
            .filter(|t| !t.chars().all(char::is_control));
        Some(Self {
            named,
            character,
            text,
            mods,
        })
    }

    /// What to send to a terminal for this key. `None` when there's nothing to send.
    pub fn to_terminal(&self) -> Option<v1::input::Kind> {
        let m = self.mods;
        if m.logo {
            return None;
        }
        if let Some(named) = self.named {
            let key = match named {
                Named::Enter => v1::Key::Enter,
                Named::Tab => v1::Key::Tab,
                Named::Backspace => v1::Key::Backspace,
                Named::Escape => v1::Key::Escape,
                Named::Space => v1::Key::Space,
                Named::Up => v1::Key::Up,
                Named::Down => v1::Key::Down,
                Named::Left => v1::Key::Left,
                Named::Right => v1::Key::Right,
                Named::Home => v1::Key::Home,
                Named::End => v1::Key::End,
                Named::PageUp => v1::Key::PageUp,
                Named::PageDown => v1::Key::PageDown,
                Named::Insert => v1::Key::Insert,
                Named::Delete => v1::Key::Delete,
                Named::F(n) => match n {
                    1 => v1::Key::F1,
                    2 => v1::Key::F2,
                    3 => v1::Key::F3,
                    4 => v1::Key::F4,
                    5 => v1::Key::F5,
                    6 => v1::Key::F6,
                    7 => v1::Key::F7,
                    8 => v1::Key::F8,
                    9 => v1::Key::F9,
                    10 => v1::Key::F10,
                    11 => v1::Key::F11,
                    _ => v1::Key::F12,
                },
            };
            return Some(v1::input::Kind::Key(v1::KeyInput {
                key: key.into(),
                text: String::new(),
                modifiers: m.bits(),
            }));
        }
        if m.ctrl || (m.alt && !cfg!(target_os = "macos")) {
            let character = self.character.clone()?;
            return Some(v1::input::Kind::Key(v1::KeyInput {
                key: v1::Key::Character.into(),
                text: character,
                modifiers: m.bits(),
            }));
        }
        // Plain typing (including Option-composed characters on macOS).
        // Some platforms (Android) report the character but no text for plain keys.
        self.text
            .clone()
            .or_else(|| self.character.clone())
            .map(v1::input::Kind::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(
        named: Option<Named>,
        character: Option<&str>,
        text: Option<&str>,
        mods: Mods,
    ) -> KeyPress {
        KeyPress {
            named,
            character: character.map(Into::into),
            text: text.map(Into::into),
            mods,
        }
    }

    #[test]
    fn typing_sends_text() {
        let k = press(None, Some("a"), Some("a"), Mods::default());
        assert_eq!(k.to_terminal(), Some(v1::input::Kind::Text("a".into())));
    }

    #[test]
    fn control_chords_send_keys() {
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let k = press(None, Some("c"), None, ctrl);
        let Some(v1::input::Kind::Key(key)) = k.to_terminal() else {
            panic!()
        };
        assert_eq!(
            (key.key, key.text.as_str(), key.modifiers),
            (v1::Key::Character as i32, "c", 2)
        );
    }

    #[test]
    fn named_keys_and_cmd() {
        let k = press(Some(Named::Up), None, None, Mods::default());
        assert!(
            matches!(k.to_terminal(), Some(v1::input::Kind::Key(v1::KeyInput { key, .. })) if key == v1::Key::Up as i32)
        );
        let cmd = press(
            None,
            Some("t"),
            Some("t"),
            Mods {
                logo: true,
                ..Mods::default()
            },
        );
        assert_eq!(cmd.to_terminal(), None, "Cmd chords are app shortcuts");
    }
}
