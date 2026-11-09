//! The task agent: the setup wizard that chooses it, the Settings rows that change it,
//! and starting threads from tasks with it.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::board::board;
use super::{attached, bodies, named};
use crate::keys::Named;
use crate::msg::{Followup, Msg};
use crate::state::{AppState, Dialog, TaskLink};
use crate::task_agent::{SetupMsg, SetupStep};

pub(super) fn settings(task_agent: v1::Agent, task_mode: v1::AgentMode) -> Msg {
    Msg::Response {
        followup: Followup::Settings,
        result: Ok(Outcome::Settings(v1::Settings {
            agents: Some(v1::AgentSettings {
                task_agent: task_agent.into(),
                task_mode: task_mode.into(),
                ..Default::default()
            }),
            ..Default::default()
        })),
    }
}

fn agents(installed: &[v1::Agent]) -> Msg {
    Msg::Response {
        followup: Followup::Agents,
        result: Ok(Outcome::Agents(v1::Agents {
            agents: installed
                .iter()
                .map(|a| v1::InstalledAgent {
                    agent: (*a).into(),
                    path: String::new(),
                })
                .collect(),
        })),
    }
}

/// Connected to a daemon of protocol `minor`.
pub(super) fn on(minor: u32) -> AppState {
    let mut s = attached();
    s.daemon_minor = minor;
    s
}

fn patch(fx: &[crate::msg::Effect]) -> v1::SettingsPatch {
    match bodies(fx)[..] {
        [Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(p) })] => p.clone(),
        _ => panic!("{fx:?}"),
    }
}

#[test]
fn the_wizard_opens_once_settings_and_agents_are_in_and_saves_the_choice() {
    let mut s = on(7);
    s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
    assert!(s.dialog.is_none(), "waits for the installed agents");
    s.update(agents(&[v1::Agent::Claude, v1::Agent::Codex]));
    assert!(matches!(
        s.dialog,
        Some(Dialog::Setup {
            step: SetupStep::Agent,
            agent: urushi_core::Agent::Claude,
            ..
        })
    ));

    s.update(Msg::Setup(SetupMsg::PickAgent(urushi_core::Agent::Codex)));
    s.update(Msg::Setup(SetupMsg::PickAgent(urushi_core::Agent::Claude)));
    s.update(Msg::Key(named(Named::Enter)));
    assert!(matches!(
        s.dialog,
        Some(Dialog::Setup {
            step: SetupStep::Mode,
            ..
        })
    ));
    s.update(Msg::Setup(SetupMsg::PickMode(urushi_core::AgentMode::Plan)));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    let p = patch(&fx);
    assert_eq!(p.task_agent, Some(v1::Agent::Claude.into()));
    assert_eq!(p.task_mode, Some(v1::AgentMode::Plan.into()));
    assert!(s.dialog.is_none());

    // Once chosen, it doesn't come back.
    s.update(settings(v1::Agent::Claude, v1::AgentMode::Plan));
    s.update(agents(&[v1::Agent::Claude]));
    assert!(s.dialog.is_none());
}

#[test]
fn a_mode_the_agent_lacks_is_saved_as_normal() {
    let mut s = on(7);
    s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Plan));
    s.update(agents(&[v1::Agent::Codex]));
    s.update(Msg::Setup(SetupMsg::Next));
    let fx = s.update(Msg::Setup(SetupMsg::Finish));
    assert_eq!(patch(&fx).task_mode, Some(v1::AgentMode::Normal.into()));
}

#[test]
fn later_or_escape_puts_the_wizard_off_until_the_next_run() {
    for close in [Msg::Setup(SetupMsg::Later), Msg::Key(named(Named::Escape))] {
        let mut s = on(7);
        s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
        s.update(agents(&[v1::Agent::Claude]));
        assert!(s.dialog.is_some());
        let fx = s.update(close);
        assert!(bodies(&fx).is_empty(), "nothing is saved");
        assert!(s.dialog.is_none());
        s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
        assert!(s.dialog.is_none(), "not again in this run");
    }
}

#[test]
fn no_wizard_without_agents_or_on_an_older_daemon() {
    let mut s = on(7);
    s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
    s.update(agents(&[]));
    assert!(s.dialog.is_none(), "nothing to choose from");

    let mut s = on(6);
    s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
    s.update(agents(&[v1::Agent::Claude]));
    assert!(s.dialog.is_none(), "a 1.6 daemon can't store the choice");
    assert!(bodies(&s.update(Msg::SetTaskAgent(urushi_core::Agent::Claude))).is_empty());
}

#[test]
fn settings_change_the_task_agent_and_mode_right_away() {
    let mut s = on(7);
    let fx = s.update(Msg::SetTaskAgent(urushi_core::Agent::Cursor));
    assert_eq!(patch(&fx).task_agent, Some(v1::Agent::Cursor.into()));
    assert_eq!(patch(&fx).task_mode, None);
    let fx = s.update(Msg::SetTaskMode(urushi_core::AgentMode::Plan));
    assert_eq!(patch(&fx).task_mode, Some(v1::AgentMode::Plan.into()));
    assert_eq!(patch(&fx).task_agent, None);
}

/// Start thread on task "a", then submit the dialog; returns the request.
fn start_thread_on_task(s: &mut AppState) -> v1::CreateThreadRequest {
    let mut workspace = super::workspace();
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::StartThread(TaskLink {
        task: "a".into(),
        subtask: None,
    }));
    assert!(matches!(s.dialog, Some(Dialog::NewThread { .. })));
    let fx = s.update(Msg::Submit);
    match bodies(&fx)[..] {
        [Body::CreateThread(r)] => r.clone(),
        _ => panic!("{fx:?}"),
    }
}

#[test]
fn a_thread_started_from_a_task_runs_the_task_agent_when_one_is_chosen() {
    let mut s = on(7);
    s.update(settings(v1::Agent::Claude, v1::AgentMode::Plan));
    let r = start_thread_on_task(&mut s);
    assert!(r.start_task_agent);
    assert_eq!(r.task_id, "a");

    // None chosen: it opens on the launcher, as before.
    let mut s = on(7);
    s.update(settings(v1::Agent::Unspecified, v1::AgentMode::Unspecified));
    assert!(!start_thread_on_task(&mut s).start_task_agent);

    // An older daemon doesn't know the flag.
    let mut s = on(6);
    s.update(settings(v1::Agent::Claude, v1::AgentMode::Plan));
    let r = start_thread_on_task(&mut s);
    assert!(!r.start_task_agent);
    assert!(r.skip_initial_tab);
}
