//! Executes CLI commands against a connected client.

use std::io::{BufRead, Write};

use anyhow::{Context, Result, bail};
use urushi_client::{Client, WorkspaceMirror, v1};

use crate::render::{print_plan, print_settings, print_tailnet, print_tree, short};
use crate::{Command, PaneCmd, ProjectCmd, SettingsCmd, Side, TabCmd, TailnetCmd, ThreadCmd};

/// Resolves an abbreviated id against the current workspace.
async fn resolve(client: &Client, id: &str) -> Result<String> {
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    mirror
        .resolve(id)
        .with_context(|| format!("no unique project, thread, tab or pane matches {id:?}"))
}

fn out(text: &str) -> Result<()> {
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
        Command::Ls => print_tree(&WorkspaceMirror::new(client.list_workspace(false).await?)),
        Command::Project(cmd) => project(client, cmd).await,
        Command::Thread(cmd) => thread(client, cmd).await,
        Command::Tab(TabCmd::New { thread, title }) => {
            let tab = client
                .create_tab(&resolve(client, &thread).await?, &title)
                .await?;
            out(&format!("tab {}", short(&tab.id)))
        }
        Command::Tab(TabCmd::Close { tab }) => {
            Ok(client.close_tab(&resolve(client, &tab).await?).await?)
        }
        Command::Pane(cmd) => pane(client, cmd).await,
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
        Command::Settings(SettingsCmd::Get) => print_settings(&client.settings().await?),
        Command::Settings(SettingsCmd::Set { key, value }) => {
            let settings = client.update_settings(patch(&key, &value)?).await?;
            print_settings(&settings)
        }
        Command::Tailnet(TailnetCmd::Status) => print_tailnet(&client.tailnet_status().await?),
        Command::Tailnet(TailnetCmd::On) => print_tailnet(&client.set_tailnet_serve(true).await?),
        Command::Tailnet(TailnetCmd::Off) => print_tailnet(&client.set_tailnet_serve(false).await?),
    }
}

async fn project(client: &Client, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::Add { path, name } => {
            let absolute = std::fs::canonicalize(&path).unwrap_or(path);
            let p = client
                .add_project(&absolute.to_string_lossy(), &name)
                .await?;
            out(&format!(
                "project {} {} ({})",
                short(&p.id),
                p.name,
                p.repo_root
            ))
        }
        ProjectCmd::Rm { project } => Ok(client
            .remove_project(&resolve(client, &project).await?)
            .await?),
        ProjectCmd::Branches { project } => {
            let b = client
                .list_branches(&resolve(client, &project).await?)
                .await?;
            for branch in b.branches {
                out(&format!(
                    "{} {branch}",
                    if branch == b.current { "*" } else { " " }
                ))?;
            }
            Ok(())
        }
    }
}

async fn thread(client: &Client, cmd: ThreadCmd) -> Result<()> {
    match cmd {
        ThreadCmd::New {
            project,
            title,
            base,
        } => {
            let t = client
                .create_thread(&resolve(client, &project).await?, &title, &base)
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
                .tab_of_pane(&pane)
                .context("pane is not in any tab")?
                .id
                .clone();
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
        other => bail!("unknown setting {other:?}"),
    }
    Ok(p)
}

fn confirm(prompt: &str) -> Result<bool> {
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}
