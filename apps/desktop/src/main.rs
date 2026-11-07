//! Urushi desktop entry point (macOS, Linux, Windows). Everything else lives in `urushi-app`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use urushi_app::{AppConfig, ConnectTarget};

/// The Urushi workspace.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Local daemon home (defaults to `URUSHI_HOME`, then the app data directory).
    #[arg(long)]
    home: Option<PathBuf>,
    /// Connect to a remote daemon on the tailnet instead, e.g. `box.tail1234.ts.net`.
    #[arg(long, conflicts_with = "home")]
    host: Option<String>,
    /// Log filter.
    #[arg(long, default_value = "warn", env = "URUSHI_LOG")]
    log: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&args.log).unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let target = match args.host {
        Some(host) => ConnectTarget::Remote(host),
        None => ConnectTarget::Local(args.home),
    };
    urushi_app::run(AppConfig::new(target))?;
    Ok(())
}
