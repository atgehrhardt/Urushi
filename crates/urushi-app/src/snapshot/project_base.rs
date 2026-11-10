//! The project-settings scenarios (ADR 0029): "Billing launch" with `main` of the
//! `urushi` repo as its parent branch and its parent-branch thread at the project's top
//! (`project-base-thread`), and its settings dialog with the branch list open
//! (`project-settings`).

use urushi_client::v1;

use crate::msg::Msg;
use crate::organize::Select;
use crate::project_settings::ProjectSettingsMsg;
use crate::state::AppState;

/// The project-settings scenarios.
pub(super) const SCENARIOS: &[&str] = &["project-base-thread", "project-settings"];

pub(super) fn show(state: &mut AppState, name: &str) {
    let mut workspace = state.workspace.raw().clone();
    if let Some(billing) = workspace.folders.iter_mut().find(|f| f.id == "f1") {
        billing.settings = Some(v1::FolderSettings {
            base_repo_id: "p".into(),
            base_branch: "main".into(),
            base_thread: Some(true),
        });
    }
    workspace.threads.push(v1::Thread {
        id: "tm".into(),
        project_id: "p".into(),
        title: "main".into(),
        primary_branch: "main".into(),
        worktree_path: "/Users/me/Dev/urushi".into(),
        folder_id: "f1".into(),
        project_base: true,
        attached_checkout: true,
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    });
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    if name == "project-settings" {
        state.update(Msg::OpenProjectSettings("f1".into()));
        state.branches = Some((
            "p".into(),
            v1::Branches {
                branches: vec!["main".into(), "develop".into()],
                current: "main".into(),
                remote_branches: vec!["origin/main".into(), "origin/release".into()],
            },
        ));
        state.update(Msg::ProjectSettings(ProjectSettingsMsg::ToggleSelect(
            Select::Base,
        )));
    }
}
