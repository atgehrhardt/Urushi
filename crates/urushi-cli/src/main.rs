//! `urushi`: a command-line client for the Urushi daemon.
//!
//! It is meant for scripting and for checking behaviour without the GUI (see
//! AGENTS.md). Ids can be abbreviated to any unique prefix or suffix (`ls` shows the last
//! 8 characters).

mod commands;
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
    /// List projects, threads, tabs and panes.
    Ls,
    /// Manage projects (git repositories).
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Manage threads (each one is a worktree).
    #[command(subcommand)]
    Thread(ThreadCmd),
    /// Manage tabs.
    #[command(subcommand)]
    Tab(TabCmd),
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
    /// Show or change settings.
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Serve the daemon on your tailnet.
    #[command(subcommand)]
    Tailnet(TailnetCmd),
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Add the repository containing PATH.
    Add {
        /// Any path inside the repository.
        path: PathBuf,
        /// Display name.
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Remove a project (it must have no threads).
    Rm {
        /// Project id.
        project: String,
    },
    /// List a project's branches.
    Branches {
        /// Project id.
        project: String,
    },
}

#[derive(Subcommand)]
enum ThreadCmd {
    /// Create a thread: a new branch and worktree, with a terminal in it.
    New {
        /// Project id.
        project: String,
        /// Title.
        #[arg(default_value = "")]
        title: String,
        /// Branch or commit to start from.
        #[arg(long, default_value = "")]
        base: String,
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
    },
    /// Close a tab.
    Close {
        /// Tab id.
        tab: String,
    },
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
    /// worktree-root, tailnet-port, allowed-logins (comma-separated), allow-tagged-nodes.
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
    let (client, _events) = connect(&cli).await?;
    commands::run(&client, cli.command).await
}
