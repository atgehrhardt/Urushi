//! The project-settings dialog (ADR 0029): opening it, its selectors and switch, and
//! Save. A Save that needs the open parent-branch thread closed asks the daemon for
//! that thread's deletion plan instead, and shows the usual warning; confirming it
//! closes the dialog at once, and the settings are sent once the daemon has closed the
//! thread (`Followup::ProjectBaseClosed`).

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::Select;
use crate::project_settings::{PendingSettings, ProjectSettingsDraft, ProjectSettingsMsg};
use crate::state::{AppState, Dialog};

impl AppState {
    /// Handles `OpenProjectSettings` and `ProjectSettings`.
    pub(super) fn project_settings_msg(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::OpenProjectSettings(project) => self.open_project_settings(&project, fx),
            Msg::ProjectSettings(change) => self.change_project_settings(change, fx),
            _ => {}
        }
    }

    fn open_project_settings(&mut self, project: &str, fx: &mut Vec<Effect>) {
        self.menu = None;
        let Some(folder) = self.workspace.project(project) else {
            return;
        };
        let draft = ProjectSettingsDraft::from_folder(folder);
        if let Some(repo) = &draft.repo {
            Self::request_branches(repo.clone(), fx);
        }
        self.open(Dialog::ProjectSettings(Box::new(draft)), None);
    }

    fn change_project_settings(&mut self, change: ProjectSettingsMsg, fx: &mut Vec<Effect>) {
        let Some(Dialog::ProjectSettings(draft)) = &mut self.dialog else {
            return;
        };
        match change {
            ProjectSettingsMsg::ToggleSelect(which) => {
                draft.open = (draft.open != Some(which)).then_some(which);
            }
            ProjectSettingsMsg::PickRepo(repo) => {
                if repo != draft.repo {
                    draft.branch = None;
                }
                draft.open = repo.as_ref().map(|_| Select::Base);
                draft.repo.clone_from(&repo);
                if let Some(repo) = repo {
                    Self::request_branches(repo, fx);
                }
            }
            ProjectSettingsMsg::PickBranch(branch) => {
                draft.branch = Some(branch);
                draft.open = None;
            }
            ProjectSettingsMsg::ToggleBaseThread => draft.base_thread = !draft.base_thread,
        }
    }

    /// Save: sends the settings, or first closes the open parent-branch thread if the
    /// change needs it closed.
    pub(super) fn submit_project_settings(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::ProjectSettings(draft)) = &self.dialog else {
            return;
        };
        if draft.repo.is_some() && draft.branch.is_none() {
            self.error = Some("Choose the parent branch, or set the repo to None.".into());
            return;
        }
        let pending = Box::new(PendingSettings {
            project: draft.project.clone(),
            settings: draft.to_settings(),
        });
        let remotes = self
            .branches
            .as_ref()
            .filter(|(r, _)| Some(r) == draft.repo.as_ref())
            .map(|(_, b)| b.remote_branches.clone())
            .unwrap_or_default();
        let closing = self
            .workspace
            .project_base_thread(&draft.project)
            .filter(|t| draft.needs_closing(t, &remotes))
            .map(|t| t.id.clone());
        match closing {
            Some(thread_id) => fx.push(request(
                Body::PlanThreadDeletion(v1::PlanThreadDeletionRequest { thread_id }),
                Followup::DeletionPlan(Some(pending)),
            )),
            None => Self::send_project_settings(*pending, Followup::CloseDialog, fx),
        }
    }

    /// The daemon answered `DeleteThread` for a parent-branch thread closed to change its
    /// project's settings: save them (the warning is long closed, so nothing else
    /// changes), or show the thread again and say why.
    pub(super) fn project_base_closed(
        &mut self,
        thread: &str,
        pending: PendingSettings,
        error: Option<String>,
        fx: &mut Vec<Effect>,
    ) {
        if error.is_some() {
            self.thread_deleted(thread, error, fx);
            return;
        }
        Self::send_project_settings(pending, Followup::None, fx);
    }

    /// Sends `UpdateFolderSettings`; `followup` closes the dialog when it is still open.
    fn send_project_settings(pending: PendingSettings, followup: Followup, fx: &mut Vec<Effect>) {
        fx.push(request(
            Body::UpdateFolderSettings(v1::UpdateFolderSettingsRequest {
                folder_id: pending.project,
                settings: Some(pending.settings),
            }),
            followup,
        ));
    }
}
