//! Typed wrappers for every request, so callers never build envelopes by hand.

use urushi_proto::v1::{self, notification, request::Body, response::Outcome};

use crate::{Client, ClientError};

pub(crate) fn unexpected(what: &str) -> ClientError {
    ClientError::Protocol(format!("unexpected response to {what}"))
}

macro_rules! expect {
    ($outcome:expr, $variant:ident, $what:literal) => {
        match $outcome {
            Outcome::$variant(value) => Ok(value),
            _ => Err(unexpected($what)),
        }
    };
}

// On the wire, `Project` is a repo and `Folder` is a project (ADR 0010). These methods
// use the user-facing names.
impl Client {
    pub(crate) async fn ok(&self, body: Body, what: &str) -> Result<(), ClientError> {
        match self.request(body).await? {
            Outcome::Ok(_) => Ok(()),
            _ => Err(unexpected(what)),
        }
    }

    /// Changes a thread's title, pin, status (empty clears) or project (empty ungroups).
    pub async fn update_thread(&self, request: v1::UpdateThreadRequest) -> Result<(), ClientError> {
        self.ok(Body::UpdateThread(request), "UpdateThread").await
    }

    /// Creates an empty project (a `Folder` on the wire).
    pub async fn create_project(&self, name: &str) -> Result<v1::Folder, ClientError> {
        let body = Body::CreateFolder(v1::CreateFolderRequest { name: name.into() });
        expect!(self.request(body).await?, Folder, "CreateFolder")
    }

    /// Renames a project.
    pub async fn rename_project(&self, project_id: &str, name: &str) -> Result<(), ClientError> {
        let body = Body::RenameFolder(v1::RenameFolderRequest {
            folder_id: project_id.into(),
            name: name.into(),
        });
        self.ok(body, "RenameFolder").await
    }

    /// Deletes a project; its threads become ungrouped.
    pub async fn delete_project(&self, project_id: &str) -> Result<(), ClientError> {
        let body = Body::DeleteFolder(v1::DeleteFolderRequest {
            folder_id: project_id.into(),
        });
        self.ok(body, "DeleteFolder").await
    }

    /// Moves a project to `index` among the others (0 is first; clamped to the end).
    /// Needs protocol 1.11.
    pub async fn move_project(&self, project_id: &str, index: u32) -> Result<(), ClientError> {
        let body = Body::MoveFolder(v1::MoveFolderRequest {
            folder_id: project_id.into(),
            index,
        });
        self.ok(body, "MoveFolder").await
    }

    /// Moves a thread to `index` in the workspace's thread list (0 is first; clamped to
    /// the end). The sidebar keeps this order inside each project, pin and status group.
    /// Needs protocol 1.21.
    pub async fn move_thread(&self, thread_id: &str, index: u32) -> Result<(), ClientError> {
        let body = Body::MoveThread(v1::MoveThreadRequest {
            thread_id: thread_id.into(),
            index,
        });
        self.ok(body, "MoveThread").await
    }

    /// Stops the daemon (it answers first). Every running terminal closes. Daemons
    /// before protocol 1.4 don't know this request; see [`crate::stop_local_daemon`].
    pub async fn shutdown_daemon(&self) -> Result<(), ClientError> {
        self.ok(
            Body::ShutdownDaemon(v1::ShutdownDaemonRequest {}),
            "ShutdownDaemon",
        )
        .await
    }

    /// The whole workspace. With `subscribe`, `WorkspaceChanged` events follow.
    pub async fn list_workspace(&self, subscribe: bool) -> Result<v1::Workspace, ClientError> {
        expect!(
            self.request(Body::ListWorkspace(v1::ListWorkspaceRequest { subscribe }))
                .await?,
            Workspace,
            "ListWorkspace"
        )
    }

    /// Adds the repository containing `path` (on the daemon's machine).
    pub async fn add_repo(&self, path: &str, name: &str) -> Result<v1::Project, ClientError> {
        let body = Body::AddProject(v1::AddProjectRequest {
            path: path.into(),
            name: name.into(),
        });
        expect!(self.request(body).await?, Project, "AddProject")
    }

    /// Removes a repo that has no threads.
    pub async fn remove_repo(&self, repo_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::RemoveProject(v1::RemoveProjectRequest {
                project_id: repo_id.into(),
            }),
            "RemoveProject",
        )
        .await
    }

    /// A repo's local and (since 1.18) remote-tracking branches.
    pub async fn list_branches(&self, repo_id: &str) -> Result<v1::Branches, ClientError> {
        let body = Body::ListBranches(v1::ListBranchesRequest {
            project_id: repo_id.into(),
        });
        expect!(self.request(body).await?, Branches, "ListBranches")
    }

    /// Creates a thread (and its worktree, and a first tab) from `base_ref`
    /// (empty = repo default), on a generated branch, ungrouped.
    pub async fn create_thread(
        &self,
        repo_id: &str,
        title: &str,
        base_ref: &str,
    ) -> Result<v1::Thread, ClientError> {
        self.create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id.into(),
            title: title.into(),
            base_ref: base_ref.into(),
            ..Default::default()
        })
        .await
    }

    /// Creates a thread with every option (named branch, no first tab, …).
    pub async fn create_thread_with(
        &self,
        request: v1::CreateThreadRequest,
    ) -> Result<v1::Thread, ClientError> {
        expect!(
            self.request(Body::CreateThread(request)).await?,
            Thread,
            "CreateThread"
        )
    }

    /// Renames a thread.
    pub async fn rename_thread(&self, thread_id: &str, title: &str) -> Result<(), ClientError> {
        self.ok(
            Body::RenameThread(v1::RenameThreadRequest {
                thread_id: thread_id.into(),
                title: title.into(),
            }),
            "RenameThread",
        )
        .await
    }

    /// What deleting a thread would destroy. Show it to the user before [`Client::delete_thread`].
    pub async fn plan_thread_deletion(
        &self,
        thread_id: &str,
    ) -> Result<v1::DeletionPlan, ClientError> {
        let body = Body::PlanThreadDeletion(v1::PlanThreadDeletionRequest {
            thread_id: thread_id.into(),
        });
        expect!(
            self.request(body).await?,
            DeletionPlan,
            "PlanThreadDeletion"
        )
    }

    /// Deletes a thread. `token` must come from a plan the user confirmed.
    pub async fn delete_thread(&self, thread_id: &str, token: &str) -> Result<(), ClientError> {
        self.ok(
            Body::DeleteThread(v1::DeleteThreadRequest {
                thread_id: thread_id.into(),
                token: token.into(),
            }),
            "DeleteThread",
        )
        .await
    }

    /// Opens a new tab in a thread, running `agent` in its shell
    /// (`v1::Agent::Unspecified` = just the shell).
    pub async fn create_tab(
        &self,
        thread_id: &str,
        title: &str,
        agent: v1::Agent,
    ) -> Result<v1::Tab, ClientError> {
        let body = Body::CreateTab(v1::CreateTabRequest {
            thread_id: thread_id.into(),
            title: title.into(),
            agent: agent.into(),
            ..Default::default()
        });
        expect!(self.request(body).await?, Tab, "CreateTab")
    }

    /// Hands a board task (or a subtask of it, at any depth) to an existing thread: links
    /// it there and opens a new tab, running the task agent with the task as its prompt
    /// when `start_task_agent` is set. Needs a 1.10 daemon; older ones ignore the task.
    pub async fn create_task_tab(
        &self,
        thread_id: &str,
        task_id: &str,
        subtask_id: &str,
        start_task_agent: bool,
    ) -> Result<v1::Tab, ClientError> {
        let body = Body::CreateTab(v1::CreateTabRequest {
            thread_id: thread_id.into(),
            task_id: task_id.into(),
            subtask_id: subtask_id.into(),
            start_task_agent,
            ..Default::default()
        });
        expect!(self.request(body).await?, Tab, "CreateTab")
    }

    /// The agent CLIs installed on the daemon's machine.
    pub async fn list_agents(&self) -> Result<Vec<v1::InstalledAgent>, ClientError> {
        let body = Body::ListAgents(v1::ListAgentsRequest {});
        expect!(self.request(body).await?, Agents, "ListAgents").map(|a| a.agents)
    }

    /// Closes a tab.
    pub async fn close_tab(&self, tab_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::CloseTab(v1::CloseTabRequest {
                tab_id: tab_id.into(),
            }),
            "CloseTab",
        )
        .await
    }

    /// Renames a tab.
    pub async fn rename_tab(&self, tab_id: &str, title: &str) -> Result<(), ClientError> {
        self.ok(
            Body::RenameTab(v1::RenameTabRequest {
                tab_id: tab_id.into(),
                title: title.into(),
            }),
            "RenameTab",
        )
        .await
    }

    /// Splits a pane. Returns the new pane's id.
    pub async fn split_pane(
        &self,
        tab_id: &str,
        pane_id: &str,
        side: v1::Direction,
    ) -> Result<String, ClientError> {
        let body = Body::SplitPane(v1::SplitPaneRequest {
            tab_id: tab_id.into(),
            pane_id: pane_id.into(),
            side: side.into(),
        });
        expect!(self.request(body).await?, PaneCreated, "SplitPane").map(|p| p.pane_id)
    }

    /// Closes a pane.
    pub async fn close_pane(&self, tab_id: &str, pane_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::ClosePane(v1::ClosePaneRequest {
                tab_id: tab_id.into(),
                pane_id: pane_id.into(),
            }),
            "ClosePane",
        )
        .await
    }

    /// Focuses a pane.
    pub async fn focus_pane(&self, tab_id: &str, pane_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::FocusPane(v1::FocusPaneRequest {
                tab_id: tab_id.into(),
                pane_id: pane_id.into(),
            }),
            "FocusPane",
        )
        .await
    }

    /// Places a divider (drag).
    pub async fn move_divider(
        &self,
        tab_id: &str,
        anchor_pane_id: &str,
        axis: v1::Axis,
        position: f64,
    ) -> Result<(), ClientError> {
        let body = Body::MoveDivider(v1::MoveDividerRequest {
            tab_id: tab_id.into(),
            anchor_pane_id: anchor_pane_id.into(),
            axis: axis.into(),
            position,
        });
        self.ok(body, "MoveDivider").await
    }

    /// Moves a pane edge (keyboard resize).
    pub async fn resize_pane_edge(
        &self,
        tab_id: &str,
        pane_id: &str,
        side: v1::Direction,
        amount: f64,
    ) -> Result<(), ClientError> {
        let body = Body::ResizePaneEdge(v1::ResizePaneEdgeRequest {
            tab_id: tab_id.into(),
            pane_id: pane_id.into(),
            side: side.into(),
            amount,
        });
        self.ok(body, "ResizePaneEdge").await
    }

    /// Equalizes a tab's panes.
    pub async fn equalize(&self, tab_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::Equalize(v1::EqualizeRequest {
                tab_id: tab_id.into(),
            }),
            "Equalize",
        )
        .await
    }

    /// Attaches to a pane. Returns the first full frame (seq 1); acknowledge it with
    /// [`Client::ack_screen`] to receive updates.
    pub async fn attach_pane(
        &self,
        pane_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<v1::ScreenUpdate, ClientError> {
        let body = Body::AttachPane(v1::AttachPaneRequest {
            pane_id: pane_id.into(),
            cols: cols.into(),
            rows: rows.into(),
        });
        expect!(self.request(body).await?, Screen, "AttachPane")
    }

    /// Stops receiving updates for a pane.
    pub async fn detach_pane(&self, pane_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::DetachPane(v1::DetachPaneRequest {
                pane_id: pane_id.into(),
            }),
            "DetachPane",
        )
        .await
    }

    /// Resizes a pane's terminal.
    pub async fn set_pane_size(
        &self,
        pane_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), ClientError> {
        let body = Body::SetPaneSize(v1::SetPaneSizeRequest {
            pane_id: pane_id.into(),
            cols: cols.into(),
            rows: rows.into(),
        });
        self.ok(body, "SetPaneSize").await
    }

    /// Plain text of a pane's visible screen.
    pub async fn read_screen_text(&self, pane_id: &str) -> Result<Vec<String>, ClientError> {
        let body = Body::ReadScreenText(v1::ReadScreenTextRequest {
            pane_id: pane_id.into(),
        });
        expect!(self.request(body).await?, ScreenText, "ReadScreenText").map(|t| t.lines)
    }

    /// Global settings.
    pub async fn settings(&self) -> Result<v1::Settings, ClientError> {
        expect!(
            self.request(Body::GetSettings(v1::GetSettingsRequest {}))
                .await?,
            Settings,
            "GetSettings"
        )
    }

    /// Changes settings. Unset patch fields are left alone.
    pub async fn update_settings(
        &self,
        patch: v1::SettingsPatch,
    ) -> Result<v1::Settings, ClientError> {
        let body = Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) });
        expect!(self.request(body).await?, Settings, "UpdateSettings")
    }

    /// Replaces a repo's setting overrides.
    pub async fn update_repo_settings(
        &self,
        repo_id: &str,
        settings: v1::ProjectSettings,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateProjectSettings(v1::UpdateProjectSettingsRequest {
            project_id: repo_id.into(),
            settings: Some(settings),
        });
        self.ok(body, "UpdateProjectSettings").await
    }

    /// Tailscale status and whether the daemon serves on the tailnet.
    pub async fn tailnet_status(&self) -> Result<v1::TailnetStatus, ClientError> {
        expect!(
            self.request(Body::TailnetStatus(v1::TailnetStatusRequest {}))
                .await?,
            TailnetStatus,
            "TailnetStatus"
        )
    }

    /// Turns serving on the tailnet on or off.
    pub async fn set_tailnet_serve(&self, serve: bool) -> Result<v1::TailnetStatus, ClientError> {
        let body = Body::SetTailnetServe(v1::SetTailnetServeRequest { serve });
        expect!(self.request(body).await?, TailnetStatus, "SetTailnetServe")
    }

    /// Acknowledges a screen update so the next one can be sent.
    pub async fn ack_screen(&self, pane_id: &str, seq: u64) -> Result<(), ClientError> {
        self.notify(notification::Body::AckScreen(v1::AckScreen {
            pane_id: pane_id.into(),
            seq,
        }))
        .await
    }

    /// Clears the attention of panes (the user has seen them). Needs protocol 1.9.
    pub async fn clear_attention(&self, pane_ids: &[String]) -> Result<(), ClientError> {
        self.ok(
            Body::ClearAttention(v1::ClearAttentionRequest {
                pane_ids: pane_ids.to_vec(),
            }),
            "ClearAttention",
        )
        .await
    }

    /// Sends input to a pane.
    pub async fn send_input(
        &self,
        pane_id: &str,
        kind: v1::input::Kind,
    ) -> Result<(), ClientError> {
        self.notify(notification::Body::Input(v1::Input {
            pane_id: pane_id.into(),
            kind: Some(kind),
        }))
        .await
    }

    /// Scrolls a pane's view (positive = back into history).
    pub async fn scroll_pane(
        &self,
        pane_id: &str,
        lines: i32,
        to_bottom: bool,
    ) -> Result<(), ClientError> {
        self.notify(notification::Body::ScrollPane(v1::ScrollPane {
            pane_id: pane_id.into(),
            lines,
            to_bottom,
        }))
        .await
    }
}
