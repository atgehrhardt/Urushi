//! Where the app's error ([`AppState::error`]) shows, and how long it stays.
//!
//! An error is only useful where the user is looking. So it is drawn on whatever is on
//! top of the window when it shows: the open dialog (next to the button that failed),
//! the quick terminal's panel, the task board, the phone's thread list, or else the
//! main area. Exactly one of those draws it.
//!
//! An error shown in a dialog was about that dialog: it goes when the dialog closes.

use crate::state::AppState;

/// The surface that draws the error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorPlace {
    /// The open dialog, between its body and its buttons.
    Dialog,
    /// The quick terminal's drop-down panel.
    QuickTerminal,
    /// The task-board panel.
    Board,
    /// The thread list, on phones when it fills the screen.
    Sidebar,
    /// The bottom of the main area, under the terminals.
    Main,
}

/// The surface on top of the window, which draws the error. The first that applies:
/// a dialog, the quick terminal, the task board, the phone's thread list, the main area.
pub fn error_place(state: &AppState) -> ErrorPlace {
    if state.dialog.is_some() {
        ErrorPlace::Dialog
    } else if state.quick.open {
        ErrorPlace::QuickTerminal
    } else if state.board.open {
        ErrorPlace::Board
    } else if state.compact && state.shows_thread_list_page() {
        ErrorPlace::Sidebar
    } else {
        ErrorPlace::Main
    }
}

/// What [`forget_dialog_error`] compares against: whether a dialog was open before an
/// update, and the error then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeforeUpdate {
    dialog_open: bool,
    error: Option<String>,
}

impl BeforeUpdate {
    /// Remembers what the dialog and error were before an update.
    pub fn of(state: &AppState) -> Self {
        Self {
            dialog_open: state.dialog.is_some(),
            error: state.error.clone(),
        }
    }
}

/// After an update: if it closed the dialog that was showing the error, the error goes
/// with it. An error the same update set (the dialog's request failed as it closed)
/// stays, and shows on the surface beneath.
pub fn forget_dialog_error(state: &mut AppState, before: &BeforeUpdate) {
    if before.dialog_open && state.dialog.is_none() && state.error == before.error {
        state.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectTarget;
    use crate::organize::Prompt;
    use crate::state::Dialog;
    use urushi_ui::{CellMetrics, TextField};

    fn state() -> AppState {
        AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        )
    }

    fn prompt() -> Dialog {
        Dialog::Prompt {
            prompt: Prompt::NewBoard,
            field: TextField::default(),
        }
    }

    #[test]
    fn the_surface_on_top_draws_the_error() {
        let mut s = state();
        assert_eq!(error_place(&s), ErrorPlace::Main);
        s.compact = true;
        assert_eq!(
            error_place(&s),
            ErrorPlace::Sidebar,
            "phones without a thread chosen show the thread list"
        );
        s.board.open = true;
        assert_eq!(error_place(&s), ErrorPlace::Board);
        s.quick.open = true;
        assert_eq!(error_place(&s), ErrorPlace::QuickTerminal);
        s.dialog = Some(prompt());
        assert_eq!(error_place(&s), ErrorPlace::Dialog);
    }

    #[test]
    fn closing_a_dialog_forgets_the_error_it_showed() {
        let mut s = state();
        s.dialog = Some(prompt());
        s.error = Some("taken".into());
        let before = BeforeUpdate::of(&s);
        s.dialog = None;
        forget_dialog_error(&mut s, &before);
        assert_eq!(s.error, None);
    }

    #[test]
    fn an_error_set_as_its_dialog_closes_stays() {
        let mut s = state();
        s.dialog = Some(prompt());
        let before = BeforeUpdate::of(&s);
        s.dialog = None;
        s.error = Some("could not restart".into());
        forget_dialog_error(&mut s, &before);
        assert_eq!(s.error.as_deref(), Some("could not restart"));
    }

    #[test]
    fn an_error_without_a_dialog_stays_until_dismissed() {
        let mut s = state();
        s.error = Some("no shell".into());
        let before = BeforeUpdate::of(&s);
        forget_dialog_error(&mut s, &before);
        assert_eq!(s.error.as_deref(), Some("no shell"));
        // A dialog that opens and stays open doesn't touch it either.
        let before = BeforeUpdate::of(&s);
        s.dialog = Some(prompt());
        forget_dialog_error(&mut s, &before);
        assert_eq!(s.error.as_deref(), Some("no shell"));
    }
}
