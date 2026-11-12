//! `urushi daemon restart|stop`: replace or stop the local daemon on purpose.
//!
//! Installing Urushi never touches the running daemon, because stopping it closes every
//! terminal. These commands are how the user chooses to pay that cost. They work on
//! daemons of any version: newer ones get `ShutdownDaemon`, older ones a SIGTERM.

use std::path::Path;

use anyhow::{Result, bail};
use urushi_client::{
    Client, StopMethod, WorkspaceMirror, ensure_local_daemon, local_daemon_endpoint,
    stop_local_daemon,
};

use crate::DaemonCmd;
use crate::commands::{confirm, out};

pub(crate) async fn run(home: Option<&Path>, host: Option<&str>, cmd: DaemonCmd) -> Result<()> {
    if host.is_some() {
        bail!("a daemon can only be restarted or stopped on its own machine");
    }
    let (restart, yes) = match cmd {
        DaemonCmd::Restart { yes } => (true, yes),
        DaemonCmd::Stop { yes } => (false, yes),
    };
    let (_, endpoint) = local_daemon_endpoint(home)?;
    if let Ok((client, _events)) = Client::connect(&endpoint, "urushi-cli").await {
        let welcome = client.welcome().clone();
        let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
        let (terminals, threads) = mirror.open_terminals();
        drop(client);
        out(&format!(
            "daemon {} (protocol {}.{}) is running {terminals} terminal(s) in {threads} thread(s).",
            welcome.daemon_version, welcome.protocol_major, welcome.protocol_minor
        ))?;
        if terminals > 0 {
            out("They will close, and whatever runs in them (agents, servers) stops.")?;
            if restart {
                out(
                    "Panes come back as they were: output, directories, programs, and agents' conversations.",
                )?;
            }
        }
        let verb = if restart { "Restart" } else { "Stop" };
        if !yes && !confirm(&format!("{verb} the daemon? [y/N] "))? {
            bail!("cancelled");
        }
        stop_local_daemon(home, StopMethod::Auto).await?;
        out("stopped")?;
    } else if !restart {
        out("no daemon is running")?;
        return Ok(());
    }
    if restart {
        let (client, _events) = ensure_local_daemon(home, "urushi-cli").await?;
        let w = client.welcome();
        out(&format!(
            "started daemon {} (protocol {}.{})",
            w.daemon_version, w.protocol_major, w.protocol_minor
        ))?;
    }
    Ok(())
}
