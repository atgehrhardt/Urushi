//! Connecting to a daemon over the LAN (ADR 0040).
//!
//! A [`LanKeyring`] holds this device's key and the hosts it paired with, in a private
//! file. [`pair_lan`] pairs with a host using the one-time code it shows, and
//! [`Client::connect_remote`] then picks the encrypted LAN channel for any host in the
//! keyring, or plain TCP to a tailnet address for anything else.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use urushi_core::{LanSettings, TailnetSettings};
use urushi_proto::endpoint::{parse_tcp, split_host_port};
use urushi_proto::lan::{self, Keypair, PairingCode, PublicKey};

use crate::{Client, ClientError, Events, platform};

/// Longest a LAN handshake may take.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// A host this device paired with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanHost {
    /// The address used to pair: an IP address or a name.
    pub host: String,
    /// Its LAN port.
    pub port: u16,
    /// The host's key, pinned when pairing.
    pub key: PublicKey,
    /// The host's name, for showing.
    pub host_name: String,
    /// When it paired, in milliseconds since the Unix epoch.
    pub paired_at_ms: u64,
}

impl LanHost {
    /// `host:port`, bracketing IPv6 addresses.
    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// This device's LAN key and the hosts it paired with.
#[derive(Clone, Debug)]
pub struct LanKeyring {
    path: PathBuf,
    keypair: Keypair,
    hosts: Vec<LanHost>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    private_key: String,
    public_key: String,
    #[serde(default)]
    hosts: Vec<StoredHost>,
}

#[derive(Serialize, Deserialize)]
struct StoredHost {
    host: String,
    port: u16,
    key: String,
    host_name: String,
    paired_at_ms: u64,
}

impl LanKeyring {
    /// The file name inside the client's data directory.
    pub const FILE: &'static str = "lan-keyring.json";

    /// The keyring in `dir`. A missing file gives a new key, written only once a host
    /// is paired.
    pub fn open(dir: &Path) -> Result<Self, ClientError> {
        let path = dir.join(Self::FILE);
        match std::fs::read(&path) {
            Ok(bytes) => decode(path, &bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                path,
                keypair: Keypair::generate()?,
                hosts: Vec::new(),
            }),
            Err(e) => Err(keyring_error(&path, &e)),
        }
    }

    /// Paired hosts, oldest first.
    pub fn hosts(&self) -> &[LanHost] {
        &self.hosts
    }

    /// This device's key, so a person can compare it with the host's device list.
    pub fn public_key(&self) -> PublicKey {
        self.keypair.public()
    }

    /// The paired host `spec` (`host` or `host:port`) names. Without a port, the most
    /// recently paired host with that address.
    pub fn find(&self, spec: &str) -> Option<&LanHost> {
        let (host, port) = split_host_port(spec)?;
        self.hosts
            .iter()
            .rev()
            .find(|h| h.host.eq_ignore_ascii_case(&host) && port.is_none_or(|p| p == h.port))
    }

    /// Forgets a paired host. Returns whether it was there.
    pub fn forget(&mut self, spec: &str) -> Result<bool, ClientError> {
        let Some(found) = self.find(spec).cloned() else {
            return Ok(false);
        };
        self.hosts.retain(|h| *h != found);
        self.save()?;
        Ok(true)
    }

    fn remember(&mut self, host: LanHost) -> Result<(), ClientError> {
        self.hosts
            .retain(|h| !(h.host.eq_ignore_ascii_case(&host.host) && h.port == host.port));
        self.hosts.push(host);
        self.save()
    }

    fn save(&self) -> Result<(), ClientError> {
        let stored = Stored {
            private_key: self.keypair.private_hex(),
            public_key: self.keypair.public().to_hex(),
            hosts: self
                .hosts
                .iter()
                .map(|h| StoredHost {
                    host: h.host.clone(),
                    port: h.port,
                    key: h.key.to_hex(),
                    host_name: h.host_name.clone(),
                    paired_at_ms: h.paired_at_ms,
                })
                .collect(),
        };
        let json =
            serde_json::to_vec_pretty(&stored).map_err(|e| ClientError::Keyring(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| keyring_error(&self.path, &e))?;
        }
        platform::write_private(&self.path, &json).map_err(|e| keyring_error(&self.path, &e))
    }
}

fn decode(path: PathBuf, bytes: &[u8]) -> Result<LanKeyring, ClientError> {
    let damaged = |e: &dyn std::fmt::Display| {
        ClientError::Keyring(format!("{} is damaged: {e}", path.display()))
    };
    let stored: Stored = serde_json::from_slice(bytes).map_err(|e| damaged(&e))?;
    let keypair =
        Keypair::from_hex(&stored.private_key, &stored.public_key).map_err(|e| damaged(&e))?;
    let hosts = stored
        .hosts
        .into_iter()
        .map(|h| {
            Ok(LanHost {
                key: PublicKey::from_hex(&h.key).map_err(|e| damaged(&e))?,
                host: h.host,
                port: h.port,
                host_name: h.host_name,
                paired_at_ms: h.paired_at_ms,
            })
        })
        .collect::<Result<_, ClientError>>()?;
    Ok(LanKeyring {
        path,
        keypair,
        hosts,
    })
}

fn keyring_error(path: &Path, e: &std::io::Error) -> ClientError {
    ClientError::Keyring(format!("cannot use {}: {e}", path.display()))
}

/// Pairs this device with the host at `spec` (`host` or `host:port`; the LAN default
/// port if none), using the code the host shows, and remembers it in `keyring`.
pub async fn pair_lan(
    spec: &str,
    code: &str,
    device_name: &str,
    keyring: &mut LanKeyring,
    now_ms: u64,
) -> Result<LanHost, ClientError> {
    let code = PairingCode::parse(code)?;
    let (host, port) = split_host_port(spec)
        .ok_or_else(|| ClientError::Protocol(format!("{spec:?} isn't a host or host:port")))?;
    let port = port.unwrap_or(LanSettings::DEFAULT_PORT);
    let stream = tcp(&host, port).await?;
    let paired = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        lan::pair(stream, &keyring.keypair, code, device_name),
    )
    .await
    .map_err(|_| timed_out(&host, port))??;
    let entry = LanHost {
        host,
        port,
        key: paired.key,
        host_name: paired.host_name,
        paired_at_ms: now_ms,
    };
    keyring.remember(entry.clone())?;
    Ok(entry)
}

impl Client {
    /// Connects to a paired LAN host over the encrypted channel.
    pub async fn connect_lan(
        host: &LanHost,
        keyring: &LanKeyring,
        client_name: &str,
    ) -> Result<(Self, Events), ClientError> {
        let stream = tcp(&host.host, host.port).await?;
        let secure = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            lan::connect(stream, &keyring.keypair, &host.key),
        )
        .await
        .map_err(|_| timed_out(&host.host, host.port))??;
        Self::over(secure, client_name).await
    }

    /// Connects to a remote daemon: over the LAN if `spec` names a host in `keyring`,
    /// otherwise over plain TCP to a tailnet address (default port 7420).
    pub async fn connect_remote(
        spec: &str,
        keyring: Option<&LanKeyring>,
        client_name: &str,
    ) -> Result<(Self, Events), ClientError> {
        if let Some(keyring) = keyring
            && let Some(host) = keyring.find(spec)
        {
            return Self::connect_lan(host, keyring, client_name).await;
        }
        let endpoint = parse_tcp(spec, TailnetSettings::DEFAULT_PORT)
            .ok_or_else(|| ClientError::Protocol(format!("{spec:?} isn't a host or host:port")))?;
        Self::connect(&endpoint, client_name)
            .await
            .map_err(|e| not_paired_hint(e, spec))
    }
}

/// A daemon that hangs up during the handshake may be a LAN host this device never
/// paired with (it expects the encrypted channel): say so.
fn not_paired_hint(error: ClientError, spec: &str) -> ClientError {
    match error {
        ClientError::Frame(_) | ClientError::Protocol(_) => ClientError::Protocol(format!(
            "{error}. If {spec} is a LAN host, this device must be paired with it first \
             (enter the pairing code it shows)."
        )),
        error => error,
    }
}

async fn tcp(host: &str, port: u16) -> Result<TcpStream, ClientError> {
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(|source| ClientError::Connect {
            endpoint: format!("{host}:{port}"),
            source,
        })?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn timed_out(host: &str, port: u16) -> ClientError {
    ClientError::Connect {
        endpoint: format!("{host}:{port}"),
        source: std::io::Error::new(std::io::ErrorKind::TimedOut, "the LAN handshake timed out"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str, port: u16) -> LanHost {
        LanHost {
            host: name.into(),
            port,
            key: Keypair::generate().unwrap().public(),
            host_name: "mac".into(),
            paired_at_ms: 1,
        }
    }

    #[test]
    fn a_keyring_is_written_only_once_something_is_paired() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyring = LanKeyring::open(dir.path()).unwrap();
        assert!(!dir.path().join(LanKeyring::FILE).exists());
        keyring.remember(host("192.168.1.10", 7421)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(LanKeyring::FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let again = LanKeyring::open(dir.path()).unwrap();
        assert_eq!(again.hosts(), keyring.hosts());
        assert_eq!(again.public_key(), keyring.public_key());
    }

    #[test]
    fn hosts_are_found_with_or_without_a_port() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyring = LanKeyring::open(dir.path()).unwrap();
        keyring.remember(host("192.168.1.10", 7421)).unwrap();
        keyring.remember(host("Mac.local", 9000)).unwrap();
        keyring.remember(host("fd12::1", 7421)).unwrap();
        assert_eq!(keyring.find("192.168.1.10").unwrap().port, 7421);
        assert_eq!(keyring.find("192.168.1.10:7421").unwrap().port, 7421);
        assert!(
            keyring.find("192.168.1.10:7420").is_none(),
            "the tailnet port"
        );
        assert_eq!(keyring.find("mac.local.").unwrap().port, 9000);
        assert_eq!(
            keyring.find("[fd12::1]:7421").unwrap().address(),
            "[fd12::1]:7421"
        );
        assert!(keyring.find("box.tail.ts.net").is_none());
        // Pairing the same address again replaces it.
        keyring.remember(host("192.168.1.10", 7421)).unwrap();
        assert_eq!(keyring.hosts().len(), 3);
        assert!(keyring.forget("192.168.1.10").unwrap());
        assert!(!keyring.forget("192.168.1.10").unwrap());
        assert_eq!(LanKeyring::open(dir.path()).unwrap().hosts().len(), 2);
    }

    #[test]
    fn a_damaged_keyring_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LanKeyring::FILE), b"nope").unwrap();
        assert!(matches!(
            LanKeyring::open(dir.path()),
            Err(ClientError::Keyring(_))
        ));
    }
}
