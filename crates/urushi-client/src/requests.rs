//! Typed wrappers for every request, so callers never build envelopes by hand.

use urushi_proto::v1::{self, notification, request::Body, response::Outcome};

use crate::{Client, ClientError};

fn unexpected(what: &str) -> ClientError {
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

impl Client {
    async fn ok(&self, body: Body, what: &str) -> Result<(), ClientError> {
        match self.request(body).await? {
            Outcome::Ok(_) => Ok(()),
            _ => Err(unexpected(what)),
        }
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
    pub async fn add_project(&self, path: &str, name: &str) -> Result<v1::Project, ClientError> {
        let body = Body::AddProject(v1::AddProjectRequest {
            path: path.into(),
            name: name.into(),
        });
        expect!(self.request(body).await?, Project, "AddProject")
    }

    /// Removes a project that has no threads.
    pub async fn remove_project(&self, project_id: &str) -> Result<(), ClientError> {
        self.ok(
            Body::RemoveProject(v1::RemoveProjectRequest {
                project_id: project_id.into(),
            }),
            "RemoveProject",
        )
        .await
    }

    /// A project's local branches.
    pub async fn list_branches(&self, project_id: &str) -> Result<v1::Branches, ClientError> {
        let body = Body::ListBranches(v1::ListBranchesRequest {
            project_id: project_id.into(),
        });
        expect!(self.request(body).await?, Branches, "ListBranches")
    }

    /// Creates a thread (and its worktree) from `base_ref` (empty = project default).
    pub async fn create_thread(
        &self,
        project_id: &str,
        title: &str,
        base_ref: &str,
    ) -> Result<v1::Thread, ClientError> {
        let body = Body::CreateThread(v1::CreateThreadRequest {
            project_id: project_id.into(),
            title: title.into(),
            base_ref: base_ref.into(),
        });
        expect!(self.request(body).await?, Thread, "CreateThread")
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

    /// Opens a new tab in a thread.
    pub async fn create_tab(&self, thread_id: &str, title: &str) -> Result<v1::Tab, ClientError> {
        let body = Body::CreateTab(v1::CreateTabRequest {
            thread_id: thread_id.into(),
            title: title.into(),
        });
        expect!(self.request(body).await?, Tab, "CreateTab")
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

    /// Replaces a project's setting overrides.
    pub async fn update_project_settings(
        &self,
        project_id: &str,
        settings: v1::ProjectSettings,
    ) -> Result<(), ClientError> {
        let body = Body::UpdateProjectSettings(v1::UpdateProjectSettingsRequest {
            project_id: project_id.into(),
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
