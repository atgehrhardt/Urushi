//! The task agent: the first-run setup wizard that chooses it, and the Settings rows
//! that change it, with how tasks get their message (sent or drafted, the default or
//! the user's own). All just send `UpdateSettings`; the daemon keeps the choice.

use urushi_client::v1::{self, request::Body};
use urushi_core::{Agent, AgentMode};
use urushi_proto::convert;

use super::request;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, Field, SettingToggle};
use crate::task_agent::{SetupMsg, SetupStep};

impl AppState {
    /// Opens the setup wizard if it is due (see [`AppState::setup_due`]). Called when the
    /// settings or the installed agents arrive.
    pub(super) fn maybe_open_setup(&mut self) {
        if !self.setup_due() {
            return;
        }
        let agent = self.agents[0];
        self.open(
            Dialog::Setup {
                step: SetupStep::Agent,
                agent,
                mode: self.task_mode_setting(),
            },
            None,
        );
    }

    /// A choice in the setup wizard.
    pub(super) fn setup_msg(&mut self, msg: SetupMsg, fx: &mut Vec<Effect>) {
        let Some(Dialog::Setup { step, agent, mode }) = &mut self.dialog else {
            return;
        };
        match msg {
            SetupMsg::PickAgent(a) => *agent = a,
            SetupMsg::PickMode(m) => *mode = m,
            SetupMsg::Next => *step = SetupStep::Mode,
            SetupMsg::Back => *step = SetupStep::Agent,
            SetupMsg::Finish => {
                let (agent, mode) = (*agent, agent.effective_mode(*mode));
                self.dialog = None;
                self.focus = None;
                save(Some(agent), Some(mode), fx);
            }
            SetupMsg::Later => {
                self.setup.dismissed = true;
                self.dialog = None;
                self.focus = None;
            }
        }
    }

    /// Enter in the wizard: on to the next step, or save on the last.
    pub(super) fn setup_enter(&mut self, fx: &mut Vec<Effect>) {
        let msg = match &self.dialog {
            Some(Dialog::Setup {
                step: SetupStep::Agent,
                ..
            }) => SetupMsg::Next,
            Some(Dialog::Setup { .. }) => SetupMsg::Finish,
            _ => return,
        };
        self.setup_msg(msg, fx);
    }

    /// Settings: start threads from tasks with `agent`.
    pub(super) fn set_task_agent(&self, agent: Agent, fx: &mut Vec<Effect>) {
        if self.daemon_supports_task_agent() {
            save(Some(agent), None, fx);
        }
    }

    /// Settings: switch between sending and drafting task messages, or the user's own
    /// task message on and off. Applied at once, like the other switches; turning the
    /// message on puts the cursor in it. Nothing on daemons older than 1.19.
    pub(super) fn toggle_task_message(&mut self, toggle: SettingToggle, fx: &mut Vec<Effect>) {
        if !self.daemon_supports_task_message() {
            return;
        }
        let mut patch = v1::SettingsPatch::default();
        match toggle {
            SettingToggle::TaskSend => {
                patch.task_draft = Some(!self.task_delivery().is_draft());
            }
            SettingToggle::TaskMessageCustom => {
                let on = !self.task_message().0;
                patch.task_message_custom = Some(on);
                if on && matches!(self.dialog, Some(Dialog::Settings { .. })) {
                    self.focus = Some(Field::TaskMessage);
                }
            }
            _ => return,
        }
        fx.push(request(
            Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
            Followup::Settings,
        ));
    }

    /// Settings: start the task agent in `mode`.
    pub(super) fn set_task_mode(&self, mode: AgentMode, fx: &mut Vec<Effect>) {
        if self.daemon_supports_task_agent() {
            save(None, Some(mode), fx);
        }
    }
}

fn save(agent: Option<Agent>, mode: Option<AgentMode>, fx: &mut Vec<Effect>) {
    let patch = v1::SettingsPatch {
        task_agent: agent.map(|a| convert::agent_to_proto(a).into()),
        task_mode: mode.map(|m| convert::agent_mode_to_proto(m).into()),
        ..Default::default()
    };
    fx.push(request(
        Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
        Followup::Settings,
    ));
}
