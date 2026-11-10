//! Project settings (ADR 0029, protocol 1.18): the dialog, saving, closing the open
//! parent-branch thread through the deletion warning first, and the new-thread dialog
//! following the project's parent branch.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::RepoId;

use super::{attached, bodies, workspace};
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{PROJECT_SETTINGS_SINCE_MINOR, Select};
use crate::project_settings::ProjectSettingsMsg as M;
use crate::state::{AppState, Dialog};

fn repo() -> String {
    RepoId::from_u128(1).to_string()
}

/// The sample workspace plus project "Billing" (`f1`) with `main` in the sample repo as
/// its parent branch, and (if `open`) its parent-branch thread `m`.
fn with_parent(open: bool) -> AppState {
    let mut s = attached();
    s.daemon_minor = PROJECT_SETTINGS_SINCE_MINOR;
    let mut w = workspace();
    w.folders.push(v1::Folder {
        id: "f1".into(),
        name: "Billing".into(),
        settings: Some(v1::FolderSettings {
            base_repo_id: repo(),
            base_branch: "main".into(),
            base_thread: Some(true),
        }),
        ..Default::default()
    });
    if open {
        w.threads.push(v1::Thread {
            id: "m".into(),
            project_id: repo(),
            title: "main".into(),
            primary_branch: "main".into(),
            folder_id: "f1".into(),
            project_base: true,
            attached_checkout: true,
            state: v1::ThreadState::Ready.into(),
            ..Default::default()
        });
    }
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

fn followups(fx: &[Effect]) -> Vec<&Followup> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Request { followup, .. } => Some(followup),
            _ => None,
        })
        .collect()
}

fn draft(s: &AppState) -> &crate::project_settings::ProjectSettingsDraft {
    match &s.dialog {
        Some(Dialog::ProjectSettings(d)) => d,
        other => panic!("not the project settings: {other:?}"),
    }
}

#[test]
fn the_dialog_shows_the_settings_and_saves_a_new_parent_branch() {
    let mut s = with_parent(false);
    let fx = s.update(Msg::OpenProjectSettings("f1".into()));
    assert!(matches!(bodies(&fx)[..], [Body::ListBranches(r)] if r.project_id == repo()));
    assert_eq!(draft(&s).branch.as_deref(), Some("main"));
    assert!(draft(&s).base_thread);

    // Picking the repo again keeps the branch; picking none clears it.
    s.update(Msg::ProjectSettings(M::PickRepo(None)));
    assert_eq!(
        (draft(&s).repo.as_ref(), draft(&s).branch.as_ref()),
        (None, None)
    );
    let fx = s.update(Msg::ProjectSettings(M::PickRepo(Some(repo()))));
    assert_eq!(bodies(&fx).len(), 1, "asks for the repo's branches");
    assert_eq!(draft(&s).open, Some(Select::Base), "on to the branch");
    // Save without a branch is refused before anything is sent.
    assert!(bodies(&s.update(Msg::Submit)).is_empty());
    assert!(s.error.is_some());

    s.update(Msg::ProjectSettings(M::PickBranch("origin/release".into())));
    s.update(Msg::ProjectSettings(M::ToggleBaseThread));
    let fx = s.update(Msg::Submit);
    let [Body::UpdateFolderSettings(r)] = bodies(&fx)[..] else {
        panic!("{fx:?}");
    };
    assert_eq!(r.folder_id, "f1");
    let sent = r.settings.as_ref().unwrap();
    assert_eq!(
        (sent.base_branch.as_str(), sent.base_thread),
        ("origin/release", Some(false))
    );
    assert!(matches!(followups(&fx)[..], [Followup::CloseDialog]));
}

#[test]
fn switching_off_an_open_parent_branch_thread_goes_through_the_warning() {
    let mut s = with_parent(true);
    s.update(Msg::OpenProjectSettings("f1".into()));
    s.update(Msg::ProjectSettings(M::ToggleBaseThread));
    let fx = s.update(Msg::Submit);
    assert!(matches!(bodies(&fx)[..], [Body::PlanThreadDeletion(r)] if r.thread_id == "m"));
    let Some(Followup::DeletionPlan(Some(pending))) = followups(&fx).first().copied().cloned()
    else {
        panic!("{fx:?}");
    };
    assert_eq!(pending.settings.base_thread, Some(false));

    // The warning shows; nothing is closed or saved until it is confirmed.
    let plan = v1::DeletionPlan {
        thread_id: "m".into(),
        token: "tok".into(),
        keeps_worktree: true,
        ..Default::default()
    };
    s.update(Msg::Response {
        followup: Followup::DeletionPlan(Some(pending.clone())),
        result: Ok(Outcome::DeletionPlan(plan)),
    });
    assert!(matches!(
        &s.dialog,
        Some(Dialog::DeleteThread { then: Some(_), .. })
    ));
    let fx = s.update(Msg::ConfirmDelete);
    assert!(matches!(bodies(&fx)[..], [Body::DeleteThread(r)] if r.token == "tok"));
    assert!(matches!(
        followups(&fx)[..],
        [Followup::ProjectBaseClosed(t, _)] if t == "m"
    ));
    // Like any deletion, the warning closes at once and nothing waits for the daemon.
    assert!(s.dialog.is_none());

    // Once it is closed, the settings are saved.
    let fx = s.update(Msg::Response {
        followup: Followup::ProjectBaseClosed("m".into(), pending.clone()),
        result: Ok(Outcome::Ok(v1::Empty {})),
    });
    assert!(matches!(bodies(&fx)[..], [Body::UpdateFolderSettings(r)] if r.folder_id == "f1"));
    assert!(matches!(followups(&fx)[..], [Followup::None]));

    // If the daemon refuses to close it, nothing is saved and the reason shows.
    let fx = s.update(Msg::Response {
        followup: Followup::ProjectBaseClosed("m".into(), pending),
        result: Err("busy".into()),
    });
    assert!(
        bodies(&fx)
            .iter()
            .all(|b| !matches!(b, Body::UpdateFolderSettings(_)))
    );
    assert!(s.error.as_deref().is_some_and(|e| e.contains("busy")));
}

#[test]
fn keeping_the_open_thread_saves_directly() {
    let mut s = with_parent(true);
    s.update(Msg::OpenProjectSettings("f1".into()));
    let fx = s.update(Msg::Submit);
    assert!(matches!(bodies(&fx)[..], [Body::UpdateFolderSettings(_)]));
}

#[test]
fn a_new_thread_in_the_project_defaults_to_its_parent_branch_repo() {
    let mut s = with_parent(false);
    s.update(Msg::OpenNewThread {
        repo: None,
        project: Some("f1".into()),
    });
    assert!(
        matches!(&s.dialog, Some(Dialog::NewThread { repo: r, base: None, .. }) if *r == repo())
    );
}
