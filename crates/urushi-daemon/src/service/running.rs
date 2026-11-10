//! Running: which panes' agents are working right now (ADR 0039).
//!
//! An agent's hooks say when it works: Claude writes Urushi's `running` signal when it
//! takes a prompt, uses a tool or starts a subagent, and `idle` when its session ends.
//! A pane stops running when its agent finishes or waits for the user (any attention),
//! when its session ends, when the user presses Esc or Ctrl+C in it (Claude runs no hook
//! when it is interrupted), and when the pane is gone. Like attention, it lives only in
//! memory, doesn't change the workspace revision, and every change is broadcast as
//! `WorkspaceChanged`, with each tab listing its running panes.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use urushi_core::{Layout, PaneId, Timestamp};
use urushi_mux::{Input, Key};
use urushi_proto::v1;

use super::state::State;
use super::{Service, lock, now};

impl Service {
    /// Marks `pane` as working (`true`) or not. Broadcasts only on a change; a pane
    /// already running keeps the time it started.
    pub(crate) fn set_running(&self, pane: PaneId, running: bool) {
        let changed = {
            let mut panes = lock(&self.running);
            if running {
                match panes.entry(pane) {
                    Entry::Occupied(_) => false,
                    Entry::Vacant(entry) => {
                        entry.insert(now());
                        true
                    }
                }
            } else {
                panes.remove(&pane).is_some()
            }
        };
        if changed {
            self.broadcast();
        }
    }

    /// The user interrupted whatever works in `pane` (Esc or Ctrl+C). Claude runs no hook
    /// then, so this is the only way to learn it stopped; if it didn't, its next tool
    /// call says it runs again.
    pub fn user_interrupted(&self, pane: PaneId) {
        self.set_running(pane, false);
    }

    /// Whether `pane`'s agent is working.
    pub fn is_running(&self, pane: PaneId) -> bool {
        lock(&self.running).contains_key(&pane)
    }

    /// Forgets panes that are gone. Their removal is broadcast by whoever removed them.
    pub(super) fn forget_running(&self, panes: &[PaneId]) {
        let mut running = lock(&self.running);
        for pane in panes {
            running.remove(pane);
        }
    }

    /// Adds each tab's (and the quick terminal's) running panes to `workspace`, which was
    /// made from `state` (same tab order).
    pub(super) fn add_running(&self, workspace: &mut v1::Workspace, state: &State) {
        let running = lock(&self.running);
        if running.is_empty() {
            return;
        }
        for (wire, tab) in workspace.tabs.iter_mut().zip(&state.tabs) {
            wire.running = wire_running(tab.layout(), &running);
        }
        if let (Some(wire), Some(quick)) = (&mut workspace.quick_terminal, &state.quick_terminal) {
            wire.running = wire_running(quick.layout(), &running);
        }
    }
}

/// Whether `input` interrupts what works in the pane: Esc, or Ctrl+C (as a key, or as
/// the raw byte on its own, as scripts send it).
pub fn is_interrupt(input: &Input) -> bool {
    match input {
        Input::Key(event) => {
            let m = event.modifiers;
            match &event.key {
                Key::Escape => !m.super_key,
                Key::Character(c) => {
                    m.ctrl && !m.alt && !m.super_key && c.eq_ignore_ascii_case("c")
                }
                _ => false,
            }
        }
        Input::Bytes(bytes) => matches!(bytes.as_slice(), [0x1b | 0x03]),
        Input::Text(text) => matches!(text.as_str(), "\u{1b}" | "\u{3}"),
        Input::Paste(_) | Input::Mouse(_) | Input::Wheel(_) => false,
    }
}

/// The running panes of `layout`, for the wire.
fn wire_running(layout: &Layout, running: &HashMap<PaneId, Timestamp>) -> Vec<v1::PaneRunning> {
    layout
        .panes()
        .into_iter()
        .filter_map(|pane| {
            let since = running.get(&pane)?;
            Some(v1::PaneRunning {
                pane_id: pane.to_string(),
                since_ms: i64::try_from(since.unix_millis()).unwrap_or(i64::MAX),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use urushi_mux::{KeyEvent, Modifiers};

    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> Input {
        Input::Key(KeyEvent { key, modifiers })
    }

    #[test]
    fn esc_and_ctrl_c_interrupt() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(is_interrupt(&key(Key::Escape, Modifiers::NONE)));
        assert!(is_interrupt(&key(Key::Character("c".into()), ctrl)));
        assert!(is_interrupt(&Input::Bytes(vec![0x03])));
        assert!(is_interrupt(&Input::Bytes(vec![0x1b])));
    }

    #[test]
    fn typing_does_not_interrupt() {
        // Messages typed while the agent works are queued, not interrupts.
        assert!(!is_interrupt(&key(
            Key::Character("c".into()),
            Modifiers::NONE
        )));
        assert!(!is_interrupt(&key(Key::Enter, Modifiers::NONE)));
        assert!(!is_interrupt(&Input::Text("hello".into())));
        assert!(!is_interrupt(&Input::Paste("\u{3}".into())));
        // An escape sequence (an arrow key sent raw) isn't a lone Esc.
        assert!(!is_interrupt(&Input::Bytes(b"\x1b[A".to_vec())));
    }
}
