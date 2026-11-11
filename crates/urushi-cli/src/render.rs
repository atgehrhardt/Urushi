//! Human-readable output.

use std::io::Write;

use anyhow::Result;
use urushi_client::{WorkspaceMirror, v1};

/// The last 8 characters of an id, which is enough to address it in the CLI.
pub(crate) fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}

pub(crate) fn print_tree(mirror: &WorkspaceMirror) -> Result<()> {
    let mut o = std::io::stdout().lock();
    if mirror.repos().is_empty() {
        writeln!(o, "no repos yet; add one with `urushi repo add <path>`")?;
    }
    for project in mirror.projects() {
        let parent = crate::project_settings::describe(mirror, project);
        writeln!(o, "project {} {}{parent}", short(&project.id), project.name)?;
    }
    for repo in mirror.repos() {
        writeln!(o, "{} {}  {}", short(&repo.id), repo.name, repo.repo_root)?;
        for thread in mirror.threads_of_repo(&repo.id) {
            let state = match v1::ThreadState::try_from(thread.state) {
                Ok(v1::ThreadState::Ready) => String::new(),
                Ok(v1::ThreadState::DeletionFailed) => {
                    format!("  [deletion failed: {}]", thread.failure_reason)
                }
                Ok(other) => format!("  [{}]", other.as_str_name()),
                Err(_) => "  [?]".into(),
            };
            let mut organization = String::new();
            if thread.project_base {
                organization.push_str("  parent branch");
                if thread.attached_checkout {
                    organization.push_str(" (in an existing checkout)");
                }
            }
            if thread.pinned {
                organization.push_str("  pinned");
            }
            if !thread.status.is_empty() {
                let _ = std::fmt::Write::write_fmt(
                    &mut organization,
                    format_args!("  status: {}", thread.status),
                );
            }
            if let Some(project) = mirror.project(&thread.folder_id) {
                let _ = std::fmt::Write::write_fmt(
                    &mut organization,
                    format_args!("  in {}", project.name),
                );
            }
            writeln!(
                o,
                "  {} {}  ({}){state}{organization}",
                short(&thread.id),
                thread.title,
                thread.primary_branch
            )?;
            for tab in mirror.tabs_of(&thread.id) {
                writeln!(o, "    {} {}", short(&tab.id), tab.title)?;
                print_panes(
                    &mut o,
                    "     ",
                    tab.layout.as_ref(),
                    &tab.focused_pane_id,
                    (&tab.attention, &tab.running),
                )?;
            }
        }
    }
    if let Some(quick) = mirror.quick_terminal_raw() {
        write_quick(&mut o, quick)?;
    }
    Ok(())
}

/// Prints the quick terminal and its panes.
pub(crate) fn print_quick(quick: &v1::QuickTerminal) -> Result<()> {
    write_quick(&mut std::io::stdout().lock(), quick)
}

fn write_quick(o: &mut impl Write, quick: &v1::QuickTerminal) -> Result<()> {
    writeln!(o, "quick terminal {}", short(&quick.id))?;
    print_panes(
        o,
        "  ",
        quick.layout.as_ref(),
        &quick.focused_pane_id,
        (&quick.attention, &quick.running),
    )
}

/// One line per pane: `*` marks the focused one, then whether its agent runs and any
/// attention.
fn print_panes(
    o: &mut impl Write,
    indent: &str,
    layout: Option<&v1::LayoutNode>,
    focused: &str,
    (attention, running): (&[v1::PaneAttention], &[v1::PaneRunning]),
) -> Result<()> {
    let Some(layout) = layout.and_then(|l| urushi_proto::convert::layout_from_proto(l).ok()) else {
        return Ok(());
    };
    for pane in layout.panes() {
        let pane = pane.to_string();
        let marker = if pane == focused { "*" } else { " " };
        let attention = attention
            .iter()
            .find(|a| a.pane_id == pane)
            .map_or("", |a| match a.kind() {
                v1::AttentionKind::Done => "  [finished]",
                _ => "  [needs input]",
            });
        let running = if running.iter().any(|r| r.pane_id == pane) {
            "  [running]"
        } else {
            ""
        };
        writeln!(
            o,
            "{indent}{marker}{} pane{running}{attention}",
            short(&pane)
        )?;
    }
    Ok(())
}

pub(crate) fn print_plan(plan: &v1::DeletionPlan) -> Result<()> {
    let mut o = std::io::stdout().lock();
    writeln!(o, "Deleting this thread will permanently remove:")?;
    if plan.keeps_worktree {
        writeln!(
            o,
            "  nothing on disk: {} is an existing checkout and is kept",
            plan.worktree_path
        )?;
    } else {
        writeln!(o, "  worktree        {}", plan.worktree_path)?;
    }
    if plan.modified_files + plan.untracked_files > 0 {
        writeln!(
            o,
            "  uncommitted     {} modified, {} untracked file(s)",
            plan.modified_files, plan.untracked_files
        )?;
    }
    if plan.unpushed_commits > 0 {
        writeln!(
            o,
            "  unpushed        {} commit(s) that exist nowhere else",
            plan.unpushed_commits
        )?;
    }
    for branch in &plan.local_branches {
        writeln!(o, "  local branch    {branch}")?;
    }
    for remote in &plan.remote_branches {
        writeln!(o, "  remote branch   {}/{}", remote.remote, remote.branch)?;
    }
    if !plan.delete_remote_branches {
        writeln!(
            o,
            "  (remote branches are kept: delete-remote-branches is off)"
        )?;
    }
    if plan.running_panes > 0 {
        writeln!(o, "  terminals       {} running", plan.running_panes)?;
    }
    for warning in &plan.warnings {
        writeln!(o, "  warning: {warning}")?;
    }
    Ok(())
}

pub(crate) fn print_settings(s: &v1::Settings) -> Result<()> {
    let mut o = std::io::stdout().lock();
    let t = s.tailnet.clone().unwrap_or_default();
    writeln!(o, "delete-remote-branches  {}", s.delete_remote_branches)?;
    writeln!(o, "branch-prefix           {}", s.branch_prefix)?;
    writeln!(
        o,
        "default-shell           {}",
        if s.default_shell.is_empty() {
            "(login shell)"
        } else {
            &s.default_shell
        }
    )?;
    writeln!(
        o,
        "worktree-root           {}",
        if s.worktree_root.is_empty() {
            "(default)"
        } else {
            &s.worktree_root
        }
    )?;
    writeln!(o, "update-base-from-remote {}", s.update_base_from_remote)?;
    writeln!(o, "tailnet-serve           {}", t.serve)?;
    writeln!(o, "tailnet-port            {}", t.port)?;
    writeln!(
        o,
        "allowed-logins          {}",
        if t.allowed_logins.is_empty() {
            "(owner only)".into()
        } else {
            t.allowed_logins.join(", ")
        }
    )?;
    writeln!(o, "allow-tagged-nodes      {}", t.allow_tagged_nodes)?;
    let lan = s.lan.unwrap_or_default();
    writeln!(o, "lan-access              {}", lan.enabled)?;
    writeln!(o, "lan-port                {}", lan.port)?;
    let a = s.agents.clone().unwrap_or_default();
    writeln!(o, "claude-skip-permissions {}", a.claude_skip_permissions)?;
    writeln!(o, "codex-skip-permissions  {}", a.codex_skip_permissions)?;
    writeln!(o, "cursor-skip-permissions {}", a.cursor_skip_permissions)?;
    let task_agent = match a.task_agent() {
        v1::Agent::Unspecified => "(not chosen)",
        v1::Agent::Claude => "claude",
        v1::Agent::Codex => "codex",
        v1::Agent::Cursor => "cursor",
    };
    writeln!(o, "task-agent              {task_agent}")?;
    let task_mode = match a.task_mode() {
        v1::AgentMode::Plan => "plan",
        v1::AgentMode::Normal | v1::AgentMode::Unspecified => "normal",
    };
    writeln!(o, "task-mode               {task_mode}")?;
    writeln!(o, "task-send               {}", !a.task_draft)?;
    writeln!(o, "task-message-custom     {}", a.task_message_custom)?;
    writeln!(
        o,
        "task-message            {}",
        if a.task_message.is_empty() {
            "(none)".to_owned()
        } else {
            a.task_message.replace('\n', "\\n")
        }
    )?;
    let (colors, logo) = crate::look::describe(s);
    writeln!(o, "colors                  {colors}")?;
    writeln!(o, "logo                    {logo}")?;
    Ok(())
}

pub(crate) fn print_tailnet(s: &v1::TailnetStatus) -> Result<()> {
    let mut o = std::io::stdout().lock();
    if !s.installed {
        writeln!(o, "tailscale: not installed")?;
    } else if !s.running {
        writeln!(o, "tailscale: not running ({})", s.error)?;
    } else {
        writeln!(
            o,
            "tailscale: {} as {} ({})",
            s.dns_name,
            s.login,
            s.addresses.join(", ")
        )?;
    }
    if s.serving {
        writeln!(o, "serving:   yes, at {}:{}", s.dns_name, s.port)?;
    } else {
        writeln!(o, "serving:   no")?;
    }
    if s.running && !s.error.is_empty() {
        writeln!(o, "error:     {}", s.error)?;
    }
    Ok(())
}
