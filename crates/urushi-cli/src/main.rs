//! `urushi`: a command-line client for the Urushi daemon.
//!
//! It is meant for scripting and for checking behaviour without the GUI (see
//! AGENTS.md). Ids can be abbreviated to any unique prefix or suffix (`ls` shows the last
//! 8 characters).

mod boards;
mod commands;
mod daemon;
mod render;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use urushi_client::{Client, Events, ensure_local_daemon, parse_tcp};
use urushi_core::TailnetSettings;

#[derive(Parser)]
#[command(
    name = "urushi",
    version,
    about = "Command-line client for the Urushi daemon"
)]
struct Cli {
    /// Local daemon home (defaults to `URUSHI_HOME`, then the app data dir).
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Connect to a remote daemon over the tailnet instead, e.g. `box.tail1234.ts.net`.
    #[arg(long, global = true)]
    host: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the daemon and tailnet status.
    Status,
    /// List repos, threads, tabs and panes.
    Ls,
    /// Manage repos (git repositories).
    #[command(subcommand)]
    Repo(RepoCmd),
    /// Manage projects (groups of threads, from any repo).
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Manage threads (each one is a worktree).
    #[command(subcommand)]
    Thread(ThreadCmd),
    /// Manage tabs.
    #[command(subcommand)]
    Tab(TabCmd),
    /// Manage task boards.
    #[command(subcommand)]
    Board(boards::BoardCmd),
    /// Show which agent runs in a pane and how much of its plan is used.
    Usage {
        /// Pane id.
        pane: String,
    },
    /// Manage panes (terminals).
    #[command(subcommand)]
    Pane(PaneCmd),
    /// Type text into a pane, followed by Enter unless --no-enter.
    Send {
        /// Pane id.
        pane: String,
        /// Text to type.
        text: Vec<String>,
        /// Don't press Enter afterwards.
        #[arg(long)]
        no_enter: bool,
    },
    /// Print a pane's visible screen.
    Screen {
        /// Pane id.
        pane: String,
    },
    /// List the agent CLIs installed on the daemon's machine.
    Agents,
    /// Restart or stop the local daemon, e.g. to switch to a newly installed version.
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// Show or change settings.
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Serve the daemon on your tailnet.
    #[command(subcommand)]
    Tailnet(TailnetCmd),
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Stop the daemon and start this installation's daemon. Every running terminal
    /// closes; threads, tabs and layouts come back with fresh shells.
    Restart {
        /// Don't ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Stop the daemon. Every running terminal closes.
    Stop {
        /// Don't ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum RepoCmd {
    /// Add the repository containing PATH.
    Add {
        /// Any path inside the repository.
        path: PathBuf,
        /// Display name.
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Remove a repo (it must have no threads).
    Rm {
        /// Repo id.
        repo: String,
    },
    /// List a repo's branches.
    Branches {
        /// Repo id.
        repo: String,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Create a project.
    New {
        /// Name.
        name: String,
    },
    /// Rename a project.
    Rename {
        /// Project id.
        project: String,
        /// New name.
        name: String,
    },
    /// Delete a project. Its threads become ungrouped; nothing on disk changes.
    Rm {
        /// Project id.
        project: String,
    },
    /// Move a project to a position in the list (0 is first).
    Move {
        /// Project id.
        project: String,
        /// New position, counted without the project (clamped to the end).
        index: u32,
    },
}

#[derive(Subcommand)]
enum ThreadCmd {
    /// Create a thread: a new branch and worktree, with a terminal in it.
    New {
        /// Repo id.
        repo: String,
        /// Title.
        #[arg(default_value = "")]
        title: String,
        /// Branch or commit to start from.
        #[arg(long, default_value = "")]
        base: String,
        /// Name of the new branch (default: `<prefix><title>-<id>`).
        #[arg(long, default_value = "")]
        branch: String,
        /// Don't open a first terminal.
        #[arg(long)]
        no_tab: bool,
        /// File it under this project.
        #[arg(long, default_value = "")]
        project: String,
        /// Start it for this board task or subtask (links them).
        #[arg(long)]
        task: Option<String>,
        /// With --task: start the task agent (settings task-agent / task-mode) with the
        /// task as its prompt.
        #[arg(long, requires = "task")]
        start_agent: bool,
    },
    /// Pin a thread (listed first in its group).
    Pin {
        /// Thread id.
        thread: String,
    },
    /// Unpin a thread.
    Unpin {
        /// Thread id.
        thread: String,
    },
    /// Set a thread's status, or clear it when STATUS is omitted.
    Status {
        /// Thread id.
        thread: String,
        /// Status label, e.g. "In review".
        #[arg(default_value = "")]
        status: String,
    },
    /// File a thread under a project, or ungroup it when PROJECT is omitted.
    Move {
        /// Thread id.
        thread: String,
        /// Project id.
        #[arg(default_value = "")]
        project: String,
    },
    /// Rename a thread.
    Rename {
        /// Thread id.
        thread: String,
        /// New title.
        title: String,
    },
    /// Delete a thread, its worktree, and every branch it owns. Shows what will be deleted
    /// and asks for confirmation.
    Rm {
        /// Thread id.
        thread: String,
        /// Skip the confirmation prompt (the plan is still printed).
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum TabCmd {
    /// Open a tab in a thread.
    New {
        /// Thread id.
        thread: String,
        /// Title.
        #[arg(long, default_value = "")]
        title: String,
        /// Agent CLI to start in the tab's shell.
        #[arg(long, value_enum)]
        agent: Option<AgentArg>,
        /// Hand this board task or subtask to the thread: links them (moving the link
        /// from any other thread) and opens the tab for it.
        #[arg(long, conflicts_with_all = ["agent", "title"])]
        task: Option<String>,
        /// With --task: start the task agent (settings task-agent / task-mode) with the
        /// task as its prompt.
        #[arg(long, requires = "task")]
        start_agent: bool,
    },
    /// Close a tab.
    Close {
        /// Tab id.
        tab: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum AgentArg {
    Claude,
    Codex,
    Cursor,
}

#[derive(Clone, Copy, ValueEnum)]
enum Side {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Subcommand)]
enum PaneCmd {
    /// Split a pane.
    Split {
        /// Pane id.
        pane: String,
        /// Where the new pane goes.
        #[arg(long, value_enum, default_value = "right")]
        side: Side,
    },
    /// Close a pane.
    Close {
        /// Pane id.
        pane: String,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Print all settings.
    Get,
    /// Change one setting: delete-remote-branches, branch-prefix, default-shell,
    /// worktree-root, tailnet-port, allowed-logins (comma-separated), allow-tagged-nodes,
    /// claude-skip-permissions, codex-skip-permissions, cursor-skip-permissions,
    /// task-agent (claude, codex, cursor or none), task-mode (normal or plan).
    Set {
        /// Setting name.
        key: String,
        /// New value.
        value: String,
    },
}

#[derive(Subcommand)]
enum TailnetCmd {
    /// Show Tailscale status and whether the daemon is serving.
    Status,
    /// Start serving on the tailnet.
    On,
    /// Stop serving on the tailnet.
    Off,
}

async fn connect(cli: &Cli) -> Result<(Client, Events)> {
    match &cli.host {
        Some(host) => {
            let endpoint =
                parse_tcp(host, TailnetSettings::DEFAULT_PORT).context("invalid --host")?;
            Client::connect(&endpoint, "urushi-cli")
                .await
                .with_context(|| format!("connecting to {endpoint}"))
        }
        None => ensure_local_daemon(cli.home.as_deref(), "urushi-cli")
            .await
            .context("connecting to the local daemon"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Restarting must not start (or keep) a daemon first.
    if let Command::Daemon(cmd) = cli.command {
        return daemon::run(cli.home.as_deref(), cli.host.as_deref(), cmd).await;
    }
    let (client, _events) = connect(&cli).await?;
    commands::run(&client, cli.command).await
}
