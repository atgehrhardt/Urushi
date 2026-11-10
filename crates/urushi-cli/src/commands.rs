//! Executes CLI commands against a connected client.

use std::io::{BufRead, Write};

use anyhow::{Context, Result, bail};
use urushi_client::{Client, WorkspaceMirror, v1};

use crate::render::{print_plan, print_quick, print_settings, print_tailnet, print_tree, short};
use crate::{
    AgentArg, Command, PaneCmd, ProjectCmd, QuickCmd, RepoCmd, SettingsCmd, Side, TabCmd,
    TailnetCmd, ThreadCmd,
};

/// Resolves an abbreviated id against the current workspace.
pub(crate) async fn resolve(client: &Client, id: &str) -> Result<String> {
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    mirror.resolve(id).with_context(|| {
        format!("no unique repo, project, thread, tab, pane, board, task or subtask matches {id:?}")
    })
}

/// The `(task_id, subtask_id)` to link for an abbreviated task or subtask id: a subtask
/// (at any depth) is linked as `subtask_id` under its card. Empty strings for none.
async fn task_link(client: &Client, task: Option<String>) -> Result<(String, String)> {
    let Some(id) = task else {
        return Ok((String::new(), String::new()));
    };
    let id = resolve(client, &id).await?;
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    Ok(match mirror.find_task(&id) {
        Some(found) if !found.task().is_card() => (found.card.id.clone(), id),
        _ => (id, String::new()),
    })
}

pub(crate) fn out(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{text}")?;
    Ok(())
}

pub(crate) async fn run(client: &Client, command: Command) -> Result<()> {
    match command {
        Command::Status => {
            let w = client.welcome();
            out(&format!(
                "daemon {} on {} (protocol {}.{})",
                w.daemon_version, w.host_name, w.protocol_major, w.protocol_minor
            ))?;
            print_tailnet(&client.tailnet_status().await?)
        }
        // Handled before connecting (see `main`).
        Command::Daemon(_) => Ok(()),
        Command::Ls => print_tree(&WorkspaceMirror::new(client.list_workspace(false).await?)),
        Command::Repo(cmd) => repo(client, cmd).await,
        Command::Project(cmd) => project(client, cmd).await,
        Command::Thread(cmd) => thread(client, cmd).await,
        Command::Tab(cmd) => tab(client, cmd).await,
        Command::Pane(cmd) => pane(client, cmd).await,
        Command::Quick(cmd) => quick(client, cmd).await,
        Command::Board(cmd) => crate::boards::run(client, cmd).await,
        Command::Jira(cmd) => crate::jira::run(client, cmd).await,
        Command::Usage { pane } => {
            crate::boards::usage(client, &resolve(client, &pane).await?).await
        }
        Command::Send {
            pane,
            text,
            no_enter,
        } => {
            let pane = resolve(client, &pane).await?;
            let mut text = text.join(" ");
            if !no_enter {
                text.push('\r');
            }
            client
                .send_input(&pane, v1::input::Kind::Text(text))
                .await?;
            // Notifications are fire-and-forget; a round trip makes sure it was delivered.
            client.read_screen_text(&pane).await?;
            Ok(())
        }
        Command::Screen { pane } => {
            let lines = client
                .read_screen_text(&resolve(client, &pane).await?)
                .await?;
            let end = lines
                .iter()
                .rposition(|l| !l.is_empty())
                .map_or(0, |i| i + 1);
            out(&lines[..end].join("\n"))
        }
        Command::Agents => {
            for installed in client.list_agents().await? {
                out(&format!(
                    "{:<8} {}",
                    installed
                        .agent()
                        .as_str_name()
                        .trim_start_matches("AGENT_")
                        .to_lowercase(),
                    installed.path
                ))?;
            }
            Ok(())
        }
        Command::Settings(SettingsCmd::Get) => print_settings(&client.settings().await?),
        Command::Settings(SettingsCmd::Set { key, value }) => {
            let settings = if crate::look::is_look(&key) {
                crate::look::set(client, &key, &value).await?
            } else {
                client.update_settings(patch(&key, &value)?).await?
            };
            print_settings(&settings)
        }
        Command::Tailnet(TailnetCmd::Status) => print_tailnet(&client.tailnet_status().await?),
        Command::Tailnet(TailnetCmd::On) => print_tailnet(&client.set_tailnet_serve(true).await?),
        Command::Tailnet(TailnetCmd::Off) => print_tailnet(&client.set_tailnet_serve(false).await?),
    }
}

async fn repo(client: &Client, cmd: RepoCmd) -> Result<()> {
    match cmd {
        RepoCmd::Add { path, name } => {
            // Absolute is enough: the daemon resolves the repo's root and canonical path itself.
            let absolute = std::path::absolute(&path).unwrap_or(path);
            let p = client.add_repo(&absolute.to_string_lossy(), &name).await?;
            out(&format!(
                "repo {} {} ({})",
                short(&p.id),
                p.name,
                p.repo_root
            ))
        }
        RepoCmd::Rm { repo } => Ok(client.remove_repo(&resolve(client, &repo).await?).await?),
        RepoCmd::Branches { repo } => {
            let b = client.list_branches(&resolve(client, &repo).await?).await?;
            for branch in b.branches {
                out(&format!(
                    "{} {branch}",
                    if branch == b.current { "*" } else { " " }
                ))?;
            }
            for branch in b.remote_branches {
                out(&format!("  {branch}  (remote)"))?;
            }
            Ok(())
        }
    }
}

async fn project(client: &Client, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::New { name } => {
            let p = client.create_project(&name).await?;
            out(&format!("project {} {}", short(&p.id), p.name))
        }
        ProjectCmd::Rename { project, name } => Ok(client
            .rename_project(&resolve(client, &project).await?, &name)
            .await?),
        ProjectCmd::Rm { project } => Ok(client
            .delete_project(&resolve(client, &project).await?)
            .await?),
        ProjectCmd::Move { project, index } => Ok(client
            .move_project(&resolve(client, &project).await?, index)
            .await?),
        ProjectCmd::Settings {
            project,
            repo,
            branch,
            clear,
            base_thread,
        } => {
            let change = crate::project_settings::Change {
                repo,
                branch,
                clear,
                base_thread: base_thread.map(|b| matches!(b, crate::OnOff::On)),
            };
            crate::project_settings::run(client, &project, change).await
        }
    }
}

/// `UpdateThread` for one thread.
async fn update_thread(
    client: &Client,
    thread: &str,
    change: impl FnOnce(&mut v1::UpdateThreadRequest),
) -> Result<()> {
    let mut request = v1::UpdateThreadRequest {
        thread_id: resolve(client, thread).await?,
        ..Default::default()
    };
    change(&mut request);
    Ok(client.update_thread(request).await?)
}

async fn tab(client: &Client, cmd: TabCmd) -> Result<()> {
    match cmd {
        TabCmd::New {
            thread,
            task: Some(task),
            start_agent,
            ..
        } => {
            let thread = resolve(client, &thread).await?;
            let (task_id, subtask_id) = task_link(client, Some(task)).await?;
            let tab = client
                .create_task_tab(&thread, &task_id, &subtask_id, start_agent)
                .await?;
            out(&format!("tab {}", short(&tab.id)))
        }
        TabCmd::New {
            thread,
            title,
            agent,
            ..
        } => {
            let agent = match agent {
                None => v1::Agent::Unspecified,
                Some(AgentArg::Claude) => v1::Agent::Claude,
                Some(AgentArg::Codex) => v1::Agent::Codex,
                Some(AgentArg::Cursor) => v1::Agent::Cursor,
            };
            let tab = client
                .create_tab(&resolve(client, &thread).await?, &title, agent)
                .await?;
            out(&format!("tab {}", short(&tab.id)))
        }
        TabCmd::Close { tab } => Ok(client.close_tab(&resolve(client, &tab).await?).await?),
    }
}

async fn thread(client: &Client, cmd: ThreadCmd) -> Result<()> {
    match cmd {
        ThreadCmd::New {
            repo,
            title,
            base,
            branch,
            no_tab,
            project,
            task,
            start_agent,
        } => {
            let folder_id = if project.is_empty() {
                String::new()
            } else {
                resolve(client, &project).await?
            };
            let (task_id, subtask_id) = task_link(client, task).await?;
            let t = client
                .create_thread_with(v1::CreateThreadRequest {
                    project_id: resolve(client, &repo).await?,
                    title,
                    base_ref: base,
                    branch,
                    skip_initial_tab: no_tab,
                    folder_id,
                    task_id,
                    subtask_id,
                    start_task_agent: start_agent,
                })
                .await?;
            out(&format!(
                "thread {} {} on {} at {}",
                short(&t.id),
                t.title,
                t.primary_branch,
                t.worktree_path
            ))
        }
        ThreadCmd::Rename { thread, title } => Ok(client
            .rename_thread(&resolve(client, &thread).await?, &title)
            .await?),
        ThreadCmd::Pin { thread } => {
            update_thread(client, &thread, |r| r.pinned = Some(true)).await
        }
        ThreadCmd::Unpin { thread } => {
            update_thread(client, &thread, |r| r.pinned = Some(false)).await
        }
        ThreadCmd::Status { thread, status } => {
            update_thread(client, &thread, |r| r.status = Some(status)).await
        }
        ThreadCmd::Move { thread, project } => {
            let folder = if project.is_empty() {
                String::new()
            } else {
                resolve(client, &project).await?
            };
            update_thread(client, &thread, |r| r.folder_id = Some(folder)).await
        }
        ThreadCmd::Rm { thread, yes } => {
            let id = resolve(client, &thread).await?;
            let plan = client.plan_thread_deletion(&id).await?;
            print_plan(&plan)?;
            if !yes && !confirm("Delete this thread and everything listed above? [y/N] ")? {
                bail!("cancelled");
            }
            client.delete_thread(&id, &plan.token).await?;
            out("deleted")
        }
    }
}

async fn pane(client: &Client, cmd: PaneCmd) -> Result<()> {
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let (pane, tab) = match &cmd {
        PaneCmd::Split { pane, .. } | PaneCmd::Close { pane } => {
            let pane = mirror
                .resolve(pane)
                .with_context(|| format!("no unique pane matches {pane:?}"))?;
            let tab = mirror
                .owner_of_pane(&pane)
                .context("pane is not in any tab")?;
            (pane, tab)
        }
    };
    match cmd {
        PaneCmd::Split { side, .. } => {
            let side = match side {
                Side::Left => v1::Direction::Left,
                Side::Right => v1::Direction::Right,
                Side::Up => v1::Direction::Up,
                Side::Down => v1::Direction::Down,
            };
            let new = client.split_pane(&tab, &pane, side).await?;
            out(&format!("pane {}", short(&new)))
        }
        PaneCmd::Close { .. } => Ok(client.close_pane(&tab, &pane).await?),
    }
}

async fn quick(client: &Client, cmd: QuickCmd) -> Result<()> {
    match cmd {
        QuickCmd::Open => print_quick(&client.open_quick_terminal().await?),
        QuickCmd::Close => {
            let mirror = client.list_workspace(false).await?;
            let quick = mirror.quick_terminal.context("no quick terminal is open")?;
            Ok(client.close_tab(&quick.id).await?)
        }
    }
}

fn patch(key: &str, value: &str) -> Result<v1::SettingsPatch> {
    let flag = |v: &str| match v {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => bail!("expected true or false, got {v:?}"),
    };
    let mut p = v1::SettingsPatch::default();
    match key {
        "delete-remote-branches" => p.delete_remote_branches = Some(flag(value)?),
        "branch-prefix" => p.branch_prefix = Some(value.into()),
        "default-shell" => p.default_shell = Some(value.into()),
        "worktree-root" => p.worktree_root = Some(value.into()),
        "update-base-from-remote" => p.update_base_from_remote = Some(flag(value)?),
        "tailnet-port" => p.tailnet_port = Some(value.parse().context("expected a port number")?),
        "allowed-logins" => {
            p.set_allowed_logins = true;
            p.allowed_logins = value
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
        }
        "allow-tagged-nodes" => p.allow_tagged_nodes = Some(flag(value)?),
        "claude-skip-permissions" => p.claude_skip_permissions = Some(flag(value)?),
        "codex-skip-permissions" => p.codex_skip_permissions = Some(flag(value)?),
        "cursor-skip-permissions" => p.cursor_skip_permissions = Some(flag(value)?),
        "task-agent" => {
            let agent = match value {
                "claude" => v1::Agent::Claude,
                "codex" => v1::Agent::Codex,
                "cursor" => v1::Agent::Cursor,
                "none" | "" => v1::Agent::Unspecified,
                _ => bail!("expected claude, codex, cursor or none, got {value:?}"),
            };
            p.task_agent = Some(agent.into());
        }
        "task-mode" => {
            let mode = match value {
                "normal" => v1::AgentMode::Normal,
                "plan" => v1::AgentMode::Plan,
                _ => bail!("expected normal or plan, got {value:?}"),
            };
            p.task_mode = Some(mode.into());
        }
        "task-send" => p.task_draft = Some(!flag(value)?),
        "task-message-custom" => p.task_message_custom = Some(flag(value)?),
        "task-message" => p.task_message = Some(value.into()),
        other => bail!("unknown setting {other:?}"),
    }
    Ok(p)
}

pub(crate) fn confirm(prompt: &str) -> Result<bool> {
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}
