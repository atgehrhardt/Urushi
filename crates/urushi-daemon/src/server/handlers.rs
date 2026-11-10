//! Request dispatch: wire request → service call → wire response.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use urushi_core::{PaneId, RepoId, TabId, ThreadId};
use urushi_mux::TermSize;
use urushi_proto::convert;
use urushi_proto::v1::{self, ErrorCode, request::Body, response::Outcome};

use super::connection::Connection;
use super::remote;
use crate::service::{DeletionPlan, ServiceError, ServiceResult, ThreadRequest};

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
        ServiceError::Forbidden(_) => ErrorCode::PermissionDenied,
        ServiceError::Git(_) => ErrorCode::GitFailed,
        ServiceError::Unavailable(_) => ErrorCode::Unavailable,
        ServiceError::Mux(_) | ServiceError::Store(_) | ServiceError::Internal(_) => {
            ErrorCode::Internal
        }
    };
    Outcome::Error(v1::Error {
        code: code.into(),
        message: error.to_string(),
    })
}

pub(super) fn parse<T: FromStr>(value: &str, field: &'static str) -> ServiceResult<T>
where
    T::Err: std::fmt::Display,
{
    convert::parse_id(value, field).map_err(|e| ServiceError::Invalid(e.to_string()))
}

/// An id that may be empty (meaning "none").
pub(super) fn optional_id<T: FromStr>(value: &str, field: &'static str) -> ServiceResult<Option<T>>
where
    T::Err: std::fmt::Display,
{
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse(value, field).map(Some)
    }
}

fn size(cols: u32, rows: u32) -> TermSize {
    let clamp = |v: u32| u16::try_from(v.clamp(1, 1000)).unwrap_or(1000);
    TermSize::new(clamp(cols), clamp(rows))
}

pub(super) const fn ok() -> Outcome {
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
        // `*Project*` on the wire is a repo (ADR 0010).
        Body::AddProject(r) => {
            let name = Some(r.name).filter(|n| !n.is_empty());
            Outcome::Project(convert::repo_to_proto(
                &service.add_repo(PathBuf::from(r.path), name).await?,
            ))
        }
        Body::RemoveProject(r) => {
            service.remove_repo(parse::<RepoId>(&r.project_id, "project_id")?)?;
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
            let request = ThreadRequest {
                title: r.title,
                base_ref: Some(r.base_ref).filter(|b| !b.trim().is_empty()),
                branch: Some(r.branch).filter(|b| !b.trim().is_empty()),
                initial_tab: !r.skip_initial_tab,
                project: optional_id(&r.folder_id, "folder_id")?,
                task: match optional_id(&r.task_id, "task_id")? {
                    Some(task) => Some((task, optional_id(&r.subtask_id, "subtask_id")?)),
                    None => None,
                },
                start_task_agent: r.start_task_agent,
            };
            let thread = service
                .create_thread(parse(&r.project_id, "project_id")?, request)
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
        Body::CreateTab(r) => {
            let thread = parse(&r.thread_id, "thread_id")?;
            let task = match optional_id(&r.task_id, "task_id")? {
                Some(task) => Some((task, optional_id(&r.subtask_id, "subtask_id")?)),
                None => None,
            };
            let tab = if task.is_some() || r.start_task_agent {
                service
                    .create_task_tab(thread, task, r.start_task_agent)
                    .await?
            } else {
                let agent = convert::agent_from_proto(r.agent);
                service.create_tab(thread, &r.title, agent).await?
            };
            Outcome::Tab(convert::tab_to_proto(&tab))
        }
        Body::ListAgents(_) => Outcome::Agents(v1::Agents {
            agents: service
                .installed_agents()
                .await?
                .into_iter()
                .map(|found| v1::InstalledAgent {
                    agent: convert::agent_to_proto(found.agent).into(),
                    path: found.path.to_string_lossy().into_owned(),
                })
                .collect(),
        }),
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
        Body::ClearAttention(r) => {
            let panes = r
                .pane_ids
                .iter()
                .filter_map(|id| convert::parse_id::<PaneId>(id, "pane_ids").ok());
            service.clear_attention(panes);
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
            let overrides = convert::repo_settings_from_proto(&r.settings.unwrap_or_default())
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            service.update_repo_settings(parse(&r.project_id, "project_id")?, overrides)?;
            ok()
        }
        Body::UpdateThread(r) => super::organize::update_thread(connection, r)?,
        Body::GetAgentUsage(r) => {
            let report = service.agent_usage(parse(&r.pane_id, "pane_id")?).await?;
            Outcome::AgentUsage(super::usage::to_proto(&report))
        }
        body @ (Body::CreateBoard(_)
        | Body::RenameBoard(_)
        | Body::DeleteBoard(_)
        | Body::CreateTask(_)
        | Body::UpdateTask(_)
        | Body::MoveTask(_)
        | Body::DeleteTask(_)
        | Body::CreateSubtask(_)
        | Body::UpdateSubtask(_)
        | Body::DeleteSubtask(_)
        | Body::CreateColumn(_)
        | Body::UpdateColumn(_)
        | Body::MoveColumn(_)
        | Body::DeleteColumn(_)
        | Body::NestTask(_)) => super::boards::route(service, body).await?,
        body @ (Body::ConnectJira(_)
        | Body::GetJiraAccount(_)
        | Body::DisconnectJira(_)
        | Body::ListJiraBoards(_)
        | Body::ImportJiraBoard(_)
        | Body::SyncBoard(_)) => super::jira::route(service, body).await?,
        body @ (Body::AddAttachment(_) | Body::GetAttachment(_) | Body::DeleteAttachment(_)) => {
            super::boards::route_attachments(service, body).await?
        }
        Body::ShutdownDaemon(_) => {
            if !connection.local {
                return Err(ServiceError::Forbidden(
                    "the daemon can only be stopped from its own machine".into(),
                ));
            }
            tracing::info!("stopping at a client's request");
            // Answer first: the reply has to leave before the listener stops.
            let service = service.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                service.request_shutdown();
            });
            ok()
        }
        Body::CreateFolder(r) => {
            Outcome::Folder(convert::project_to_proto(&service.create_project(&r.name)?))
        }
        Body::RenameFolder(r) => {
            service.rename_project(parse(&r.folder_id, "folder_id")?, &r.name)?;
            ok()
        }
        Body::DeleteFolder(r) => {
            service.delete_project(parse(&r.folder_id, "folder_id")?)?;
            ok()
        }
        Body::MoveFolder(r) => {
            let index = usize::try_from(r.index).unwrap_or(usize::MAX);
            service.move_project(parse(&r.folder_id, "folder_id")?, index)?;
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
