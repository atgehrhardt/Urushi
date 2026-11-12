//! `urushi update check|install`: the macOS app's updates from the command line
//! (ADR 0049), the same as the app's update notice.
//!
//! Installing replaces the app on disk and then restarts the daemon, which closes every
//! terminal; restore points bring the panes back. So, like `urushi daemon restart`, it
//! says what will stop and asks first.

use std::path::Path;

use anyhow::{Result, bail};
use clap::Subcommand;
use urushi_client::{
    StopMethod, UPDATES_SINCE_MINOR, WorkspaceMirror, ensure_local_daemon, stop_local_daemon,
};

use crate::commands::{confirm, out};

#[derive(Subcommand)]
pub(crate) enum UpdateCmd {
    /// Ask GitHub for the latest release and compare it with the installed app.
    Check,
    /// Install the latest release in place of the app, then restart the daemon. Running
    /// agents stop and are reopened; quit and reopen the app afterwards.
    Install {
        /// Don't ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

pub(crate) async fn run(home: Option<&Path>, host: Option<&str>, cmd: UpdateCmd) -> Result<()> {
    let (installing, ask) = match cmd {
        UpdateCmd::Check => (false, false),
        UpdateCmd::Install { .. } if host.is_some() => {
            bail!("updates can only be installed on the daemon's own machine")
        }
        UpdateCmd::Install { yes } => (true, !yes),
    };
    let (client, _events) = crate::connect(home, host).await?;
    if client.welcome().protocol_minor < UPDATES_SINCE_MINOR {
        bail!(
            "this daemon ({}) can't update the app; restart it first (urushi daemon restart)",
            client.welcome().daemon_version
        );
    }
    let check = client.check_for_updates().await?;
    if !check.available {
        return out(&format!(
            "Urushi {} is up to date (latest release: {})",
            check.current_version, check.latest_version
        ));
    }
    out(&format!(
        "Urushi {} is available (installed: {}): {}",
        check.latest_version, check.current_version, check.release_url
    ))?;
    if !installing {
        return out("Install it with: urushi update install");
    }

    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let (terminals, threads) = mirror.open_terminals();
    let agents = mirror.running_agents();
    if agents > 0 {
        out(&format!(
            "{agents} agent(s) are working right now. Updating stops them; they're reopened \
             after the restart, but an agent in the middle of a task may need to be asked to \
             continue."
        ))?;
    }
    if terminals > 0 {
        out(&format!(
            "{terminals} terminal(s) in {threads} thread(s) close while the daemon restarts; \
             their output, directories and programs come back."
        ))?;
    }
    if ask
        && !confirm(&format!(
            "Update to {} and restart the daemon? [y/N] ",
            check.latest_version
        ))?
    {
        bail!("cancelled");
    }
    out("downloading…")?;
    let installed = client.install_update(&check.latest_version).await?;
    out(&format!(
        "installed Urushi {} in {}",
        installed.version, installed.bundle_path
    ))?;
    drop(client);
    stop_local_daemon(home, StopMethod::Auto).await?;
    let (client, _events) = ensure_local_daemon(home, "urushi-cli").await?;
    out(&format!(
        "started daemon {}; quit and reopen Urushi to use the new app",
        client.welcome().daemon_version
    ))?;
    Ok(())
}
