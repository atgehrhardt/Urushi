//! `urushi project settings`: a project's parent branch and its parent-branch thread
//! (ADR 0029).

use anyhow::{Result, bail};
use urushi_client::{Client, ClientError, WorkspaceMirror, v1};

use crate::commands::{out, resolve};
use crate::render::short;

/// What the user asked to change. Everything unset is kept.
pub(crate) struct Change {
    pub(crate) repo: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) clear: bool,
    pub(crate) base_thread: Option<bool>,
}

impl Change {
    fn is_empty(&self) -> bool {
        self.repo.is_none() && self.branch.is_none() && !self.clear && self.base_thread.is_none()
    }
}

/// Prints a project's settings, or changes them and prints the result.
pub(crate) async fn run(client: &Client, project: &str, change: Change) -> Result<()> {
    let id = resolve(client, project).await?;
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let Some(folder) = mirror.project(&id) else {
        bail!("{project:?} is not a project");
    };
    if change.is_empty() {
        return print(&mirror, folder);
    }
    let mut settings = folder.settings.clone().unwrap_or_default();
    if change.clear {
        settings.base_repo_id.clear();
        settings.base_branch.clear();
    }
    if let Some(repo) = change.repo {
        settings.base_repo_id = resolve(client, &repo).await?;
    }
    if let Some(branch) = change.branch {
        settings.base_branch = branch;
    }
    if settings.base_repo_id.is_empty() != settings.base_branch.is_empty() {
        bail!("a parent branch needs both --repo and --branch");
    }
    if let Some(on) = change.base_thread {
        settings.base_thread = Some(on);
    }
    match client.update_project_settings(&id, settings).await {
        Ok(folder) => {
            let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
            print(&mirror, &folder)
        }
        Err(ClientError::Remote {
            code: v1::ErrorCode::FailedPrecondition,
            message,
        }) => {
            let hint = mirror
                .project_base_thread(&id)
                .map(|t| format!(" (`urushi thread rm {}`)", short(&t.id)))
                .unwrap_or_default();
            bail!("{message}{hint}")
        }
        Err(e) => Err(e.into()),
    }
}

fn print(mirror: &WorkspaceMirror, folder: &v1::Folder) -> Result<()> {
    let settings = folder.settings.clone().unwrap_or_default();
    out(&format!("project {} {}", short(&folder.id), folder.name))?;
    match mirror.repo(&settings.base_repo_id) {
        Some(repo) if !settings.base_branch.is_empty() => out(&format!(
            "  parent branch       {} in {}",
            settings.base_branch, repo.name
        ))?,
        _ => out("  parent branch       none (threads start from their repo's default)")?,
    }
    let on = settings.base_thread.unwrap_or(true);
    out(&format!(
        "  parent-branch thread {}",
        match (on, mirror.project_base_thread(&folder.id)) {
            (_, Some(t)) => format!("open ({})", short(&t.id)),
            (true, None) => "on".into(),
            (false, None) => "off".into(),
        }
    ))
}

/// `  parent: <branch> in <repo>` for `urushi ls`, or nothing.
pub(crate) fn describe(mirror: &WorkspaceMirror, folder: &v1::Folder) -> String {
    let Some(settings) = folder.settings.as_ref() else {
        return String::new();
    };
    match mirror.repo(&settings.base_repo_id) {
        Some(repo) if !settings.base_branch.is_empty() => {
            format!("  (parent: {} in {})", settings.base_branch, repo.name)
        }
        _ => String::new(),
    }
}
