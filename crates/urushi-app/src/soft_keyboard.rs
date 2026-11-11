//! When a touch device's on-screen keyboard should be up.
//!
//! The keyboard opens by itself when a text field takes focus (the connect dialog's host
//! field, for one). Terminals get it only once the user taps one, so the screen stays free
//! for reading output until they want to type. It closes when nothing can take the typing:
//! the phone's thread list, or a dialog without a text field. The runner turns the answers
//! into OS calls; this module only decides, so it's tested without a window.

use crate::state::AppState;

/// What the app last asked the OS for, and whether the user asked to type into a terminal.
#[derive(Debug, Default)]
pub(crate) struct SoftKeyboard {
    /// The keyboard was last asked to show.
    shown: bool,
    /// A tap landed on a terminal, and it can still take typing.
    terminal: bool,
}

impl SoftKeyboard {
    /// A tap was released, on a terminal or elsewhere. A terminal tap asks for the
    /// keyboard; the next [`Self::sync`] should be forced, so a keyboard the user put away
    /// comes back when they tap the field or terminal again.
    pub(crate) fn tapped(&mut self, on_terminal: bool) {
        if on_terminal {
            self.terminal = true;
        }
    }

    /// What to tell the OS now: `Some(true)` to show the keyboard, `Some(false)` to hide it,
    /// `None` to leave it. Answers on a change, or when `force`d (a tap, the window gaining
    /// focus, the app resuming) and the keyboard is wanted: the OS may have hidden it
    /// without telling the app.
    pub(crate) fn sync(&mut self, state: &AppState, force: bool) -> Option<bool> {
        let terminal = terminal_can_type(state);
        if !terminal {
            self.terminal = false;
        }
        let wanted = state.field().is_some() || self.terminal && terminal;
        let answer = (wanted != self.shown || force && wanted).then_some(wanted);
        self.shown = wanted;
        answer
    }
}

/// Whether typing would reach a terminal: no dialog or board is over it, a pane has focus,
/// and on a phone the terminals (not the thread list) are on screen.
fn terminal_can_type(state: &AppState) -> bool {
    state.keyboard_surface().is_some()
        && state.focused_pane().is_some()
        && !(state.compact && state.shows_thread_list_page())
}
