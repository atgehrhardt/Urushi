//! The launcher: choosing what a new tab runs (a terminal, or an installed agent CLI).
//!
//! It appears as a picker dialog for the "+" button and the new-tab shortcut, and inline
//! in a thread that has no tabs (a new thread, or one whose last tab was closed). The
//! daemon types the agent's command into a shell (ADR 0009), so the app only names the
//! agent.

use urushi_client::v1::{self, request::Body};
use urushi_proto::convert;

use super::request;
use crate::keys::{KeyPress, Named};
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, Launch};

/// First protocol minor version with agent launching (`ListAgents`, `CreateTab.agent`).
const AGENTS_SINCE_MINOR: u32 = 1;

impl AppState {
    /// Asks the daemon which agents are installed, if it knows how to say.
    pub(super) fn request_agents(&self, fx: &mut Vec<Effect>) {
        if self.daemon_minor >= AGENTS_SINCE_MINOR {
            fx.push(request(
                Body::ListAgents(v1::ListAgentsRequest {}),
                Followup::Agents,
            ));
        }
    }

    /// Remembers the installed agents from a `ListAgents` answer.
    pub(super) fn set_agents(&mut self, agents: &v1::Agents) {
        self.agents = agents
            .agents
            .iter()
            .filter_map(|a| convert::agent_from_proto(a.agent))
            .collect();
        self.setup.agents_loaded = true;
    }

    /// Opens the picker for a new tab in the current thread.
    pub(super) fn open_launcher(&mut self, fx: &mut Vec<Effect>) {
        if self.selected_thread.is_some() {
            self.dialog = Some(Dialog::NewTab { highlighted: 0 });
            self.focus = None;
            self.request_agents(fx);
        }
    }

    /// Opens a tab in the current thread running `choice`.
    pub(super) fn launch(&mut self, choice: Launch, fx: &mut Vec<Effect>) {
        if matches!(self.dialog, Some(Dialog::NewTab { .. })) {
            self.dialog = None;
        }
        let Some(thread_id) = self.selected_thread.clone() else {
            return;
        };
        let agent = match choice {
            Launch::Terminal => v1::Agent::Unspecified,
            Launch::Agent(agent) => convert::agent_to_proto(agent),
        };
        fx.push(request(
            Body::CreateTab(v1::CreateTabRequest {
                thread_id,
                title: String::new(),
                agent: agent.into(),
            }),
            Followup::TabCreated,
        ));
    }

    /// Keys in the picker dialog: arrows move, Enter or a digit picks, Escape closes.
    pub(super) fn launcher_dialog_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        let choices = self.launch_choices();
        let Some(Dialog::NewTab { highlighted }) = &mut self.dialog else {
            return;
        };
        match key.named {
            Some(Named::Escape) => {
                self.dialog = None;
            }
            Some(Named::Up | Named::Left) => {
                *highlighted = highlighted.checked_sub(1).unwrap_or(choices.len() - 1);
            }
            Some(Named::Down | Named::Right | Named::Tab) => {
                *highlighted = (*highlighted + 1) % choices.len();
            }
            Some(Named::Enter) => {
                let choice = choices[(*highlighted).min(choices.len() - 1)];
                self.launch(choice, fx);
            }
            _ => {
                if let Some(choice) = digit_choice(key, &choices) {
                    self.launch(choice, fx);
                }
            }
        }
    }

    /// Keys in a thread with no tabs: a digit picks from the inline launcher, Enter opens
    /// a terminal. Returns whether the key was used.
    pub(super) fn empty_thread_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) -> bool {
        if self.empty_thread().is_none() {
            return false;
        }
        let choices = self.launch_choices();
        let choice = match key.named {
            Some(Named::Enter) => Some(Launch::Terminal),
            _ => digit_choice(key, &choices),
        };
        if let Some(choice) = choice {
            self.launch(choice, fx);
        }
        true
    }
}

/// The choice numbered by a digit key (1-based, as the pickers show).
fn digit_choice(key: &KeyPress, choices: &[Launch]) -> Option<Launch> {
    let digit = key.character.as_deref()?.parse::<usize>().ok()?;
    choices.get(digit.checked_sub(1)?).copied()
}
