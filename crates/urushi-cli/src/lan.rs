//! `urushi lan …`: LAN access (ADR 0040).
//!
//! On the host: switch LAN access on or off, show a pairing code, list and revoke paired
//! devices. On another machine: pair this CLI with a host (`lan pair`); `--host` then
//! connects to it over the encrypted LAN channel.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use urushi_client::{Client, LanKeyring, pair_lan, v1};

#[derive(Subcommand)]
pub(crate) enum LanCmd {
    /// Show whether LAN access is on, its addresses and the paired devices.
    Status,
    /// Let paired devices connect over this machine's private LAN addresses. Only
    /// encrypted connections from devices you pair are accepted, and nothing is ever
    /// opened on your router or firewall.
    On,
    /// Stop LAN access and close every LAN connection.
    Off,
    /// Show a one-time code (valid 10 minutes) for pairing a device.
    PairCode,
    /// Withdraw the pairing code.
    Cancel,
    /// Forget a paired device and close its connections.
    Revoke {
        /// The device's id (or a unique part of it), from `lan status`.
        device: String,
    },
    /// Pair this CLI with a host, using the code it shows.
    Pair {
        /// The host's LAN address, `host` or `host:port` (default port 7421).
        host: String,
        /// The pairing code shown on the host.
        code: String,
        /// The name the host lists this device under (default: this machine's name).
        #[arg(long)]
        name: Option<String>,
    },
    /// List the hosts this CLI has paired with.
    Hosts,
    /// Forget a paired host.
    Forget {
        /// `host` or `host:port`, as paired.
        host: String,
    },
}

impl LanCmd {
    /// Whether the command works on this CLI's keyring rather than a daemon.
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::Pair { .. } | Self::Hosts | Self::Forget { .. })
    }
}

/// Where this CLI keeps its LAN keyring: the home it would use for a local daemon.
pub(crate) fn keyring_dir(home: Option<&Path>) -> Result<PathBuf> {
    home.map(Path::to_path_buf)
        .or_else(urushi_client::home_dir)
        .context("no home directory for the LAN keyring; set URUSHI_HOME")
}

/// Runs a command that needs no daemon connection.
pub(crate) async fn run_local(home: Option<&Path>, cmd: LanCmd) -> Result<()> {
    let mut keyring = LanKeyring::open(&keyring_dir(home)?)?;
    let mut o = std::io::stdout().lock();
    match cmd {
        LanCmd::Pair { host, code, name } => {
            let name = name.unwrap_or_else(machine_name);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            let paired = pair_lan(&host, &code, &name, &mut keyring, now).await?;
            writeln!(
                o,
                "paired with {} at {} as {name:?}",
                paired.host_name,
                paired.address()
            )?;
            writeln!(o, "host key:  {}", paired.key.fingerprint())?;
            writeln!(o, "this key:  {}", keyring.public_key().fingerprint())?;
            writeln!(o, "connect with: urushi --host {} ls", paired.address())?;
        }
        LanCmd::Hosts => {
            if keyring.hosts().is_empty() {
                writeln!(o, "no paired hosts")?;
            }
            for host in keyring.hosts() {
                writeln!(
                    o,
                    "{:<24} {:<20} {}",
                    host.address(),
                    host.host_name,
                    host.key.fingerprint()
                )?;
            }
        }
        LanCmd::Forget { host } => {
            if !keyring.forget(&host)? {
                bail!("not paired with {host}");
            }
            writeln!(o, "forgot {host}")?;
        }
        _ => bail!("this command needs a daemon"),
    }
    Ok(())
}

/// Runs a command against the daemon.
pub(crate) async fn run(client: &Client, cmd: LanCmd) -> Result<()> {
    let status = match cmd {
        LanCmd::Status => client.lan_status().await?,
        LanCmd::On => client.set_lan_enabled(true).await?,
        LanCmd::Off => client.set_lan_enabled(false).await?,
        LanCmd::PairCode => client.start_lan_pairing().await?,
        LanCmd::Cancel => client.cancel_lan_pairing().await?,
        LanCmd::Revoke { device } => {
            let status = client.lan_status().await?;
            let id = find_device(&status, &device)?;
            client.revoke_lan_device(&id).await?
        }
        local => return run_local(None, local).await,
    };
    print_status(&status)
}

fn find_device(status: &v1::LanStatus, wanted: &str) -> Result<String> {
    let matches: Vec<&v1::LanDevice> = status
        .devices
        .iter()
        .filter(|d| d.id == wanted || d.id.contains(wanted))
        .collect();
    match matches.as_slice() {
        [device] => Ok(device.id.clone()),
        [] => bail!("no paired device matches {wanted:?}"),
        _ => bail!("{wanted:?} matches more than one device"),
    }
}

fn print_status(s: &v1::LanStatus) -> Result<()> {
    let mut o = std::io::stdout().lock();
    if !s.enabled {
        writeln!(o, "LAN access: off")?;
    } else if s.serving {
        let addresses: Vec<String> = s
            .addresses
            .iter()
            .map(|a| {
                if a.contains(':') {
                    format!("[{a}]:{}", s.port)
                } else {
                    format!("{a}:{}", s.port)
                }
            })
            .collect();
        writeln!(o, "LAN access: on, at {}", addresses.join(", "))?;
    } else {
        writeln!(o, "LAN access: on, not listening")?;
    }
    if !s.error.is_empty() {
        writeln!(o, "error:      {}", s.error)?;
    }
    if !s.fingerprint.is_empty() {
        writeln!(o, "host key:   {}", s.fingerprint)?;
    }
    if let Some(pairing) = &s.pairing {
        writeln!(
            o,
            "pairing code: {}  (expires in {} min; on the device: urushi lan pair <address> <code>)",
            pairing.code,
            pairing.expires_in_secs.div_ceil(60)
        )?;
    }
    if s.devices.is_empty() {
        writeln!(o, "no paired devices")?;
    }
    for d in &s.devices {
        writeln!(
            o,
            "{}  {:<20} {}  {}",
            d.id,
            d.name,
            d.fingerprint,
            if d.connected { "connected" } else { "" }
        )?;
    }
    Ok(())
}

/// This machine's name, for the host's device list.
fn machine_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "urushi CLI".into())
}
