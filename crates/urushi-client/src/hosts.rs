//! The remote hosts this device connected to, most recent first (ADR 0043).
//!
//! Mobile apps reconnect to the first at launch and offer the rest in the connect
//! dialog. The file holds addresses only: a tailnet host needs no secret, and a LAN
//! host's keys stay in the [`LanKeyring`](crate::LanKeyring).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ClientError, platform};

/// The remote hosts this device connected to, most recent first, kept in a private
/// file in the client's data directory.
#[derive(Clone, Debug)]
pub struct RecentHosts {
    path: PathBuf,
    hosts: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    hosts: Vec<String>,
}

impl RecentHosts {
    /// The file name inside the client's data directory.
    pub const FILE: &'static str = "recent-hosts.json";
    /// How many hosts are kept.
    pub const MAX: usize = 8;

    /// The hosts remembered in `dir`. A missing file is an empty list.
    pub fn open(dir: &Path) -> Result<Self, ClientError> {
        let path = dir.join(Self::FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let stored: Stored = serde_json::from_slice(&bytes).map_err(|e| {
                    ClientError::RecentHosts(format!("{} is damaged: {e}", path.display()))
                })?;
                Ok(Self {
                    path,
                    hosts: stored.hosts,
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                path,
                hosts: Vec::new(),
            }),
            Err(e) => Err(io_error(&path, &e)),
        }
    }

    /// The hosts, most recent first: each an address as it was connected to
    /// (`host[:port]`).
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    /// The host connected to last.
    pub fn last(&self) -> Option<&str> {
        self.hosts.first().map(String::as_str)
    }

    /// Records a successful connection to `address`: it moves to the front, and the
    /// oldest host beyond [`Self::MAX`] is dropped. Writes only when the list changed.
    pub fn remember(&mut self, address: &str) -> Result<(), ClientError> {
        let address = address.trim();
        if address.is_empty() || self.last() == Some(address) {
            return Ok(());
        }
        self.hosts.retain(|h| !h.eq_ignore_ascii_case(address));
        self.hosts.insert(0, address.to_owned());
        self.hosts.truncate(Self::MAX);
        self.save()
    }

    /// Forgets `address`. Returns whether it was there.
    pub fn forget(&mut self, address: &str) -> Result<bool, ClientError> {
        let before = self.hosts.len();
        self.hosts
            .retain(|h| !h.eq_ignore_ascii_case(address.trim()));
        if self.hosts.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> Result<(), ClientError> {
        let stored = Stored {
            hosts: self.hosts.clone(),
        };
        let json = serde_json::to_vec_pretty(&stored)
            .map_err(|e| ClientError::RecentHosts(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_error(&self.path, &e))?;
        }
        platform::write_private(&self.path, &json).map_err(|e| io_error(&self.path, &e))
    }
}

fn io_error(path: &Path, e: &std::io::Error) -> ClientError {
    ClientError::RecentHosts(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_is_no_hosts_and_nothing_is_written_until_one_connects() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = RecentHosts::open(dir.path()).unwrap();
        assert!(recent.hosts().is_empty());
        assert_eq!(recent.last(), None);
        assert!(!dir.path().join(RecentHosts::FILE).exists());
        recent.remember("box.tail.ts.net").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(RecentHosts::FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let again = RecentHosts::open(dir.path()).unwrap();
        assert_eq!(again.last(), Some("box.tail.ts.net"));
    }

    #[test]
    fn the_latest_host_comes_first_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = RecentHosts::open(dir.path()).unwrap();
        recent.remember("box.tail.ts.net").unwrap();
        recent.remember("192.168.1.10:7421").unwrap();
        recent.remember("Box.tail.ts.net").unwrap();
        assert_eq!(recent.hosts(), ["Box.tail.ts.net", "192.168.1.10:7421"]);
        recent.remember("  ").unwrap();
        assert_eq!(recent.hosts().len(), 2);
        assert_eq!(
            RecentHosts::open(dir.path()).unwrap().hosts(),
            recent.hosts()
        );
    }

    #[test]
    fn only_the_newest_hosts_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = RecentHosts::open(dir.path()).unwrap();
        for n in 0..RecentHosts::MAX + 3 {
            recent.remember(&format!("host{n}")).unwrap();
        }
        assert_eq!(recent.hosts().len(), RecentHosts::MAX);
        assert_eq!(
            recent.last(),
            Some(format!("host{}", RecentHosts::MAX + 2).as_str())
        );
        assert!(!recent.hosts().contains(&"host0".to_owned()));
    }

    #[test]
    fn a_host_can_be_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = RecentHosts::open(dir.path()).unwrap();
        recent.remember("a").unwrap();
        recent.remember("b").unwrap();
        assert!(recent.forget("A").unwrap());
        assert!(!recent.forget("a").unwrap());
        assert_eq!(RecentHosts::open(dir.path()).unwrap().hosts(), ["b"]);
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(RecentHosts::FILE), b"nope").unwrap();
        assert!(matches!(
            RecentHosts::open(dir.path()),
            Err(ClientError::RecentHosts(_))
        ));
    }
}
