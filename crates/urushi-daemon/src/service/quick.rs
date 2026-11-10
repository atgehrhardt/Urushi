//! The quick terminal (ADR 0031): split panes that belong to no thread. The daemon has
//! at most one; it is stored and restored like a tab, and ends with its last pane.

use urushi_core::{PaneId, QuickTerminal, TabId};
use urushi_mux::TermSize;

use super::{PaneHome, Service, ServiceResult, new_uuid};

impl Service {
    /// The quick terminal, first opening one with a shell in the home directory if there
    /// is none. Asking again returns the same one, so clients can open it freely.
    pub fn open_quick_terminal(&self) -> ServiceResult<QuickTerminal> {
        let existing = self.read(|state| Ok(state.quick_terminal.clone()))?;
        if let Some(existing) = existing {
            return Ok(existing);
        }
        let shell = self.read(|state| Ok(state.settings.default_shell.clone()))?;
        let pane = PaneId::from_uuid(new_uuid());
        self.spawn_session(pane, PaneHome::Quick, shell, TermSize::DEFAULT)?;
        // Another client may have opened one while the shell started: keep theirs.
        let result = self.mutate(|state| {
            if let Some(theirs) = &state.quick_terminal {
                return Ok((theirs.clone(), false));
            }
            let quick = QuickTerminal::new(TabId::from_uuid(new_uuid()), pane);
            state.quick_terminal = Some(quick.clone());
            Ok((quick, true))
        });
        match result {
            Ok((quick, true)) => Ok(quick),
            Ok((quick, false)) => {
                self.kill_sessions([pane]);
                Ok(quick)
            }
            Err(e) => {
                self.kill_sessions([pane]);
                Err(e)
            }
        }
    }
}
