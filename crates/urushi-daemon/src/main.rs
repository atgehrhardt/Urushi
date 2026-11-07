//! `urushid` entry point: parse flags, set up logging, run the daemon.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use urushi_daemon::{DaemonOptions, run, shutdown_signal};

/// Urushi's background service. Normally started automatically by the app or the CLI.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Home directory (state, socket, worktrees). Defaults to `URUSHI_HOME`, then the
    /// platform's app data directory.
    #[arg(long)]
    home: Option<PathBuf>,
    /// Log filter, e.g. `info` or `urushi_daemon=debug`.
    #[arg(long, default_value = "info", env = "URUSHI_LOG")]
    log: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&args.log).context("invalid --log filter")?,
        )
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .init();

    let home = args
        .home
        .or_else(urushi_proto::endpoint::home_dir)
        .context("no home directory: pass --home or set URUSHI_HOME")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(DaemonOptions::new(home), shutdown_signal()))?;
    Ok(())
}
