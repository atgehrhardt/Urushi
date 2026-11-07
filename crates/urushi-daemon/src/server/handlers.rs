//! Request dispatch: wire request → service call → wire response.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use urushi_core::{PaneId, ProjectId, TabId, ThreadId};
use urushi_mux::TermSize;
use urushi_proto::convert;
use urushi_proto::v1::{self, ErrorCode, request::Body, response::Outcome};

use super::connection::Connection;
use super::remote;
use crate::service::{DeletionPlan, ServiceError, ServiceResult};

/// An error outcome for a malformed request.
pub(crate) fn invalid(message: &str) -> Outcome {
    Outcome::Error(v1::Error {
        code: ErrorCode::InvalidArgument.into(),
        message: message.to_owned(),
    })
}

fn error_outcome(error: &ServiceError) -> Outcome {
    let code = match error {
        ServiceError::Invalid(_) => ErrorCode::InvalidArgument,
        ServiceError::NotFound(_) => ErrorCode::NotFound,
        ServiceError::Precondition(_) => ErrorCode::FailedPrecondition,
        ServiceError::Conflict(_) => ErrorCode::Conflict,
        ServiceError::Git(_) => ErrorCode::GitFailed,
        ServiceError::Mux(_) | ServiceError::Store(_) | ServiceError::Internal(_) => {
            ErrorCode::Internal
        }
    };
    Outcome::Error(v1::Error {
        code: code.into(),
        message: error.to_string(),
    })
}

fn parse<T: FromStr>(value: &str, field: &'static str) -> ServiceResult<T>
where
    T::Err: std::fmt::Display,
{
    convert::parse_id(value, field).map_err(|e| ServiceError::Invalid(e.to_string()))
}

fn size(cols: u32, rows: u32) -> TermSize {
    let clamp = |v: u32| u16::try_from(v.clamp(1, 1000)).unwrap_or(1000);
    TermSize::new(clamp(cols), clamp(rows))
}

const fn ok() -> Outcome {
    Outcome::Ok(v1::Empty {})
}

/// Handles one request. Never fails: errors become `Error` outcomes.
pub(crate) async fn handle(connection: &Arc<Connection>, body: Body) -> Outcome {
    match route(connection, body).await {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::debug!(%error, "request failed");
            error_outcome(&error)
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per request kind keeps routing in one place"
)]
async fn route(connection: &Arc<Connection>, body: Body) -> ServiceResult<Outcome> {
    let service = &connection.service;
    Ok(match body {
        Body::Hello(_) => return Err(ServiceError::Invalid("already said hello".into())),
        Body::ListWorkspace(r) => {
            if r.subscribe {
                connection.subscribe_workspace();
            }
            Outcome::Workspace(service.workspace())
        }
        Body::AddProject(r) => {
            let name = Some(r.name).filter(|n| !n.is_empty());
            Outcome::Project(convert::project_to_proto(
                &service.add_project(PathBuf::from(r.path), name).await?,
            ))
        }
        Body::RemoveProject(r) => {
            service.remove_project(parse::<ProjectId>(&r.project_id, "project_id")?)?;
            ok()
        }
        Body::ListBranches(r) => {
            let (branches, current) = service
                .list_branches(parse(&r.project_id, "project_id")?)
                .await?;
            Outcome::Branches(v1::Branches {
                branches,
                current: current.unwrap_or_default(),
            })
        }
        Body::CreateThread(r) => {
            let base = Some(r.base_ref).filter(|b| !b.is_empty());
            let thread = service
                .create_thread(parse(&r.project_id, "project_id")?, &r.title, base)
                .await?;
            Outcome::Thread(convert::thread_to_proto(&thread))
        }
        Body::RenameThread(r) => {
            service.rename_thread(parse::<ThreadId>(&r.thread_id, "thread_id")?, &r.title)?;
            ok()
        }
        Body::PlanThreadDeletion(r) => {
            let plan = service
                .plan_thread_deletion(parse(&r.thread_id, "thread_id")?)
                .await?;
            Outcome::DeletionPlan(plan_to_proto(&plan))
        }
        Body::DeleteThread(r) => {
            if r.token.is_empty() {
                return Err(ServiceError::Invalid(
                    "a deletion token from PlanThreadDeletion is required".into(),
                ));
            }
            service
                .delete_thread(parse(&r.thread_id, "thread_id")?, &r.token)
                .await?;
            ok()
        }
        Body::CreateTab(r) => Outcome::Tab(convert::tab_to_proto(
            &service.create_tab(parse(&r.thread_id, "thread_id")?, &r.title)?,
        )),
        Body::CloseTab(r) => {
            service.close_tab(parse::<TabId>(&r.tab_id, "tab_id")?)?;
            ok()
        }
        Body::RenameTab(r) => {
            service.rename_tab(parse(&r.tab_id, "tab_id")?, &r.title)?;
            ok()
        }
        Body::SplitPane(r) => {
            let side = convert::direction_from_proto(r.side)
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            let pane = service.split_pane(
                parse(&r.tab_id, "tab_id")?,
                parse(&r.pane_id, "pane_id")?,
                side,
            )?;
            Outcome::PaneCreated(v1::PaneCreated {
                tab_id: r.tab_id,
                pane_id: pane.to_string(),
            })
        }
        Body::ClosePane(r) => {
            service.close_pane(parse(&r.tab_id, "tab_id")?, parse(&r.pane_id, "pane_id")?)?;
            ok()
        }
        Body::FocusPane(r) => {
            service.focus_pane(parse(&r.tab_id, "tab_id")?, parse(&r.pane_id, "pane_id")?)?;
            ok()
        }
        Body::MoveDivider(r) => {
            let axis = convert::axis_from_proto(r.axis)
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            service.move_divider(
                parse(&r.tab_id, "tab_id")?,
                parse(&r.anchor_pane_id, "anchor_pane_id")?,
                axis,
                r.position,
            )?;
            ok()
        }
        Body::ResizePaneEdge(r) => {
            let side = convert::direction_from_proto(r.side)
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            service.resize_pane_edge(
                parse(&r.tab_id, "tab_id")?,
                parse(&r.pane_id, "pane_id")?,
                side,
                r.amount,
            )?;
            ok()
        }
        Body::Equalize(r) => {
            service.equalize(parse(&r.tab_id, "tab_id")?)?;
            ok()
        }
        Body::AttachPane(r) => {
            let pane: PaneId = parse(&r.pane_id, "pane_id")?;
            Outcome::Screen(connection.attach(pane, size(r.cols, r.rows)).await?)
        }
        Body::DetachPane(r) => {
            connection.detach(parse(&r.pane_id, "pane_id")?);
            ok()
        }
        Body::SetPaneSize(r) => {
            let session = service.session(parse(&r.pane_id, "pane_id")?)?;
            session.resize(size(r.cols, r.rows)).await?;
            ok()
        }
        Body::ReadScreenText(r) => {
            let session = service.session(parse(&r.pane_id, "pane_id")?)?;
            Outcome::ScreenText(v1::ScreenText {
                lines: session.text().await?,
            })
        }
        Body::GetSettings(_) => Outcome::Settings(convert::settings_to_proto(&service.settings())),
        Body::UpdateSettings(r) => {
            let before = service.settings().tailnet;
            let settings = service.update_settings(&r.patch.unwrap_or_default())?;
            if settings.tailnet != before {
                remote::reconcile(service).await;
            }
            Outcome::Settings(convert::settings_to_proto(&settings))
        }
        Body::UpdateProjectSettings(r) => {
            let overrides = convert::project_settings_from_proto(&r.settings.unwrap_or_default())
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            service.update_project_settings(parse(&r.project_id, "project_id")?, overrides)?;
            ok()
        }
        Body::TailnetStatus(_) => Outcome::TailnetStatus(remote::status(service).await),
        Body::SetTailnetServe(r) => {
            service.update_settings(&v1::SettingsPatch {
                tailnet_serve: Some(r.serve),
                ..Default::default()
            })?;
            remote::reconcile(service).await;
            Outcome::TailnetStatus(remote::status(service).await)
        }
    })
}

fn plan_to_proto(plan: &DeletionPlan) -> v1::DeletionPlan {
    v1::DeletionPlan {
        thread_id: plan.thread.to_string(),
        worktree_path: plan.worktree_path.to_string_lossy().into_owned(),
        modified_files: plan.changes.modified,
        untracked_files: plan.changes.untracked,
        unpushed_commits: plan.unpushed_commits,
        local_branches: plan
            .local_branches
            .iter()
            .map(ToString::to_string)
            .collect(),
        remote_branches: plan
            .remote_branches
            .iter()
            .map(|(remote, branch)| v1::RemoteBranch {
                remote: remote.clone(),
                branch: branch.clone(),
            })
            .collect(),
        running_panes: plan.running_panes,
        delete_remote_branches: plan.delete_remote_branches,
        token: plan.token.clone(),
        warnings: plan.warnings.clone(),
    }
}
