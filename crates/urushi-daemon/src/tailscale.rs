//! Talking to the installed Tailscale client (ADR 0005).
//!
//! [`TailscaleProvider`] is everything remote access needs: this machine's tailnet
//! identity and addresses, and who is behind a connecting tailnet address. [`CliTailscale`]
//! reads both from the `tailscale` CLI's JSON output. Methods are synchronous. Call
//! them from `spawn_blocking`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;
use thiserror::Error;

/// Tailscale failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TailscaleError {
    /// No `tailscale` binary found.
    #[error("Tailscale is not installed")]
    NotInstalled,
    /// Installed, but not running or not logged in.
    #[error("Tailscale is not running (state: {0})")]
    NotRunning(String),
    /// The CLI failed or printed something unexpected.
    #[error("tailscale CLI error: {0}")]
    Cli(String),
}

/// Another device on the tailnet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Host name.
    pub name: String,
    /// `MagicDNS` name, without the trailing dot.
    pub dns_name: String,
    /// Tailnet addresses.
    pub addresses: Vec<IpAddr>,
    /// Currently reachable.
    pub online: bool,
    /// Operating system.
    pub os: String,
}

/// This machine on the tailnet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// `MagicDNS` name, without the trailing dot.
    pub dns_name: String,
    /// This machine's tailnet addresses.
    pub addresses: Vec<IpAddr>,
    /// Login of the user who owns this machine.
    pub login: String,
    /// Other devices.
    pub peers: Vec<Peer>,
}

/// Who is behind a tailnet address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The owning user's login (e.g. `me@example.com`).
    pub login: String,
    /// The device's name.
    pub node: String,
    /// ACL tags, for devices owned by tags rather than users.
    pub tags: Vec<String>,
}

/// Access to Tailscale.
pub trait TailscaleProvider: Send + Sync {
    /// This machine's status. Fails if Tailscale is missing or not running.
    fn status(&self) -> Result<Status, TailscaleError>;
    /// Identifies the device at `addr`.
    fn whois(&self, addr: IpAddr) -> Result<Identity, TailscaleError>;
}

/// [`TailscaleProvider`] using the `tailscale` CLI.
#[derive(Clone, Debug, Default)]
pub struct CliTailscale {
    explicit: Option<PathBuf>,
}

impl CliTailscale {
    /// Uses a specific binary instead of searching for one.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            explicit: Some(path.into()),
        }
    }

    fn binary(&self) -> Option<PathBuf> {
        if let Some(path) = &self.explicit {
            return Some(path.clone());
        }
        let candidates = [
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
            "/usr/local/bin/tailscale",
            "/usr/bin/tailscale",
            "/opt/homebrew/bin/tailscale",
            r"C:\Program Files\Tailscale\tailscale.exe",
        ];
        let on_path = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .flat_map(|dir| [dir.join("tailscale"), dir.join("tailscale.exe")]);
        on_path
            .chain(candidates.iter().map(PathBuf::from))
            .find(|p| p.is_file())
    }

    fn json(&self, args: &[&str]) -> Result<String, TailscaleError> {
        let binary = self.binary().ok_or(TailscaleError::NotInstalled)?;
        let out = Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| TailscaleError::Cli(e.to_string()))?;
        if !out.status.success() {
            return Err(TailscaleError::Cli(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl TailscaleProvider for CliTailscale {
    fn status(&self) -> Result<Status, TailscaleError> {
        parse_status(&self.json(&["status", "--json"])?)
    }

    fn whois(&self, addr: IpAddr) -> Result<Identity, TailscaleError> {
        parse_whois(&self.json(&["whois", "--json", &addr.to_string()])?)
    }
}

/// Used when remote access isn't configured or in tests: every call fails with `error`,
/// unless `status`/`identities` are given.
#[derive(Clone, Debug)]
pub struct StaticTailscale {
    /// Returned by `status()`, or the error.
    pub status: Result<Status, TailscaleError>,
    /// Identities by address for `whois()`.
    pub identities: HashMap<IpAddr, Identity>,
}

impl StaticTailscale {
    /// A provider that reports Tailscale as not installed.
    pub fn not_installed() -> Self {
        Self {
            status: Err(TailscaleError::NotInstalled),
            identities: HashMap::new(),
        }
    }
}

impl TailscaleProvider for StaticTailscale {
    fn status(&self) -> Result<Status, TailscaleError> {
        self.status.clone()
    }

    fn whois(&self, addr: IpAddr) -> Result<Identity, TailscaleError> {
        self.identities
            .get(&addr)
            .cloned()
            .ok_or_else(|| TailscaleError::Cli(format!("no identity for {addr}")))
    }
}

// ---- JSON shapes (only the fields we use; everything else is ignored) ---------------

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatusJson {
    backend_state: String,
    #[serde(rename = "Self")]
    me: Option<NodeJson>,
    #[serde(default)]
    peer: Option<HashMap<String, NodeJson>>,
    #[serde(default)]
    user: Option<HashMap<String, UserJson>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NodeJson {
    #[serde(default)]
    host_name: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Option<Vec<String>>,
    #[serde(default)]
    online: bool,
    #[serde(rename = "OS", default)]
    os: String,
    #[serde(rename = "UserID", default)]
    user_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserJson {
    #[serde(default)]
    login_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WhoisJson {
    node: WhoisNode,
    user_profile: Option<UserJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WhoisNode {
    #[serde(default)]
    name: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

fn addresses(ips: Option<&Vec<String>>) -> Vec<IpAddr> {
    ips.map(|v| v.iter().filter_map(|ip| ip.parse().ok()).collect())
        .unwrap_or_default()
}

fn parse_status(json: &str) -> Result<Status, TailscaleError> {
    let parsed: StatusJson =
        serde_json::from_str(json).map_err(|e| TailscaleError::Cli(e.to_string()))?;
    if parsed.backend_state != "Running" {
        return Err(TailscaleError::NotRunning(parsed.backend_state));
    }
    let me = parsed
        .me
        .ok_or_else(|| TailscaleError::Cli("status has no Self".into()))?;
    let login = parsed
        .user
        .as_ref()
        .and_then(|users| users.get(&me.user_id.to_string()))
        .map(|u| u.login_name.clone())
        .unwrap_or_default();
    let mut peers: Vec<Peer> = parsed
        .peer
        .unwrap_or_default()
        .into_values()
        .map(|p| Peer {
            name: p.host_name,
            dns_name: p.dns_name.trim_end_matches('.').to_owned(),
            addresses: addresses(p.tailscale_ips.as_ref()),
            online: p.online,
            os: p.os,
        })
        .collect();
    peers.sort_by(|a, b| a.dns_name.cmp(&b.dns_name));
    Ok(Status {
        dns_name: me.dns_name.trim_end_matches('.').to_owned(),
        addresses: addresses(me.tailscale_ips.as_ref()),
        login,
        peers,
    })
}

fn parse_whois(json: &str) -> Result<Identity, TailscaleError> {
    let parsed: WhoisJson =
        serde_json::from_str(json).map_err(|e| TailscaleError::Cli(e.to_string()))?;
    Ok(Identity {
        login: parsed
            .user_profile
            .map(|u| u.login_name)
            .unwrap_or_default(),
        node: parsed.node.name.trim_end_matches('.').to_owned(),
        tags: parsed.node.tags.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"{
        "BackendState": "Running",
        "Self": {"HostName": "mac", "DNSName": "mac.tail1.ts.net.", "TailscaleIPs": ["100.1.2.3", "fd7a::1"], "UserID": 42, "Online": true, "OS": "macOS"},
        "Peer": {"k1": {"HostName": "phone", "DNSName": "phone.tail1.ts.net.", "TailscaleIPs": ["100.9.9.9"], "Online": false, "OS": "iOS", "UserID": 42}},
        "User": {"42": {"LoginName": "me@example.com"}},
        "Unknown": [1, 2, 3]
    }"#;

    #[test]
    fn parses_status() {
        let s = parse_status(STATUS).unwrap();
        assert_eq!(s.dns_name, "mac.tail1.ts.net");
        assert_eq!(s.login, "me@example.com");
        assert_eq!(s.addresses.len(), 2);
        assert_eq!(s.peers[0].name, "phone");
        assert!(!s.peers[0].online);
    }

    #[test]
    fn not_running_is_an_error() {
        let err = parse_status(r#"{"BackendState": "NeedsLogin", "Self": null}"#).unwrap_err();
        assert_eq!(err, TailscaleError::NotRunning("NeedsLogin".into()));
    }

    #[test]
    fn parses_whois_users_and_tags() {
        let user = parse_whois(r#"{"Node": {"Name": "phone.tail1.ts.net.", "Tags": null}, "UserProfile": {"LoginName": "me@example.com"}}"#).unwrap();
        assert_eq!(
            user,
            Identity {
                login: "me@example.com".into(),
                node: "phone.tail1.ts.net".into(),
                tags: vec![]
            }
        );
        let tagged = parse_whois(r#"{"Node": {"Name": "ci.", "Tags": ["tag:ci"]}, "UserProfile": {"LoginName": "tagged-devices"}}"#).unwrap();
        assert_eq!(tagged.tags, vec!["tag:ci"]);
    }
}
