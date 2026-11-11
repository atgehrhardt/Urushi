//! The daemon's LAN key and the devices paired with it.
//!
//! Kept in `<home>/lan.json`, readable only by the user (0600, in the 0700 home), and
//! replaced atomically. The private key never leaves this file: clients learn only
//! fingerprints and device names. The file is created the first time LAN access is
//! turned on, not before.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use urushi_core::Timestamp;
use urushi_proto::lan::{Keypair, LanError, PublicKey};

/// The most devices that can be paired at once.
pub const MAX_DEVICES: usize = 64;

/// A device paired with this daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Stable id, for revoking.
    pub id: String,
    /// The name it gave when it paired.
    pub name: String,
    /// Its key: only this key is accepted for it.
    pub key: PublicKey,
    /// When it paired.
    pub paired_at: Timestamp,
    /// When it last connected.
    pub last_seen: Option<Timestamp>,
}

/// The key and the paired devices.
#[derive(Clone, Debug)]
pub struct Identity {
    /// The daemon's long-term key.
    pub keypair: Keypair,
    /// Paired devices, oldest first.
    pub devices: Vec<Device>,
}

impl Identity {
    /// The device with `key`, if paired.
    pub fn device(&self, key: &PublicKey) -> Option<&Device> {
        self.devices.iter().find(|d| d.key == *key)
    }

    /// Adds a device, or renames it if its key is already paired. Fails when too many
    /// devices are paired.
    pub fn pair(
        &mut self,
        id: String,
        name: &str,
        key: PublicKey,
        now: Timestamp,
    ) -> Result<&Device, String> {
        let name = if name.is_empty() {
            "Unnamed device"
        } else {
            name
        }
        .to_owned();
        if let Some(index) = self.devices.iter().position(|d| d.key == key) {
            let device = &mut self.devices[index];
            device.name = name;
            device.paired_at = now;
            return Ok(&self.devices[index]);
        }
        if self.devices.len() >= MAX_DEVICES {
            return Err(format!(
                "at most {MAX_DEVICES} devices can be paired; revoke one first"
            ));
        }
        self.devices.push(Device {
            id,
            name,
            key,
            paired_at: now,
            last_seen: None,
        });
        Ok(&self.devices[self.devices.len() - 1])
    }

    /// Forgets a device. Returns whether it was paired.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        self.devices.len() != before
    }
}

/// Where the identity is kept.
#[derive(Clone, Debug)]
pub struct IdentityStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    private_key: String,
    public_key: String,
    #[serde(default)]
    devices: Vec<StoredDevice>,
}

#[derive(Serialize, Deserialize)]
struct StoredDevice {
    id: String,
    name: String,
    key: String,
    paired_at_ms: u64,
    #[serde(default)]
    last_seen_ms: Option<u64>,
}

/// Why the identity couldn't be loaded or saved.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// Reading or writing the file failed.
    #[error("cannot use the LAN key file: {0}")]
    Io(#[from] std::io::Error),
    /// The file isn't what we wrote.
    #[error("the LAN key file is damaged: {0}")]
    Damaged(String),
    /// A key couldn't be made.
    #[error(transparent)]
    Key(#[from] LanError),
}

impl IdentityStore {
    /// A store at `<home>/lan.json`.
    pub fn in_home(home: &Path) -> Self {
        Self {
            path: home.join("lan.json"),
        }
    }

    /// The saved identity, or a new one (saved at once) if there is none yet.
    pub fn load_or_create(&self) -> Result<Identity, IdentityError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => decode(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let identity = Identity {
                    keypair: Keypair::generate()?,
                    devices: Vec::new(),
                };
                self.save(&identity)?;
                Ok(identity)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Saves the identity, readable only by this user.
    pub fn save(&self, identity: &Identity) -> Result<(), IdentityError> {
        let stored = StoredIdentity {
            private_key: identity.keypair.private_hex(),
            public_key: identity.keypair.public().to_hex(),
            devices: identity
                .devices
                .iter()
                .map(|d| StoredDevice {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    key: d.key.to_hex(),
                    paired_at_ms: d.paired_at.unix_millis(),
                    last_seen_ms: d.last_seen.map(Timestamp::unix_millis),
                })
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&stored).map_err(std::io::Error::other)?;
        crate::platform::write_private(&self.path, &json)?;
        Ok(())
    }
}

fn decode(bytes: &[u8]) -> Result<Identity, IdentityError> {
    let stored: StoredIdentity =
        serde_json::from_slice(bytes).map_err(|e| IdentityError::Damaged(e.to_string()))?;
    let keypair = Keypair::from_hex(&stored.private_key, &stored.public_key)
        .map_err(|e| IdentityError::Damaged(e.to_string()))?;
    let devices = stored
        .devices
        .into_iter()
        .map(|d| {
            Ok(Device {
                id: d.id,
                name: d.name,
                key: PublicKey::from_hex(&d.key)
                    .map_err(|e| IdentityError::Damaged(e.to_string()))?,
                paired_at: Timestamp::from_unix_millis(d.paired_at_ms),
                last_seen: d.last_seen_ms.map(Timestamp::from_unix_millis),
            })
        })
        .collect::<Result<_, IdentityError>>()?;
    Ok(Identity { keypair, devices })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PublicKey {
        Keypair::generate().unwrap().public()
    }

    #[test]
    fn identity_is_created_once_saved_privately_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::in_home(dir.path());
        let mut identity = store.load_or_create().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("lan.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let phone = key();
        identity
            .pair("d1".into(), "phone", phone, Timestamp::from_unix_millis(5))
            .unwrap();
        identity.devices[0].last_seen = Some(Timestamp::from_unix_millis(9));
        store.save(&identity).unwrap();

        let again = store.load_or_create().unwrap();
        assert_eq!(again.keypair.public(), identity.keypair.public());
        assert_eq!(again.keypair.private_hex(), identity.keypair.private_hex());
        assert_eq!(again.devices, identity.devices);
        assert_eq!(again.device(&phone).unwrap().name, "phone");
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_new_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lan.json"), b"{not json").unwrap();
        let store = IdentityStore::in_home(dir.path());
        assert!(matches!(
            store.load_or_create(),
            Err(IdentityError::Damaged(_))
        ));
    }

    #[test]
    fn pairing_again_renames_and_revoking_forgets() {
        let mut identity = Identity {
            keypair: Keypair::generate().unwrap(),
            devices: Vec::new(),
        };
        let phone = key();
        let t = Timestamp::from_unix_millis(1);
        identity.pair("d1".into(), "phone", phone, t).unwrap();
        identity.pair("d2".into(), "new phone", phone, t).unwrap();
        assert_eq!(identity.devices.len(), 1);
        assert_eq!(identity.devices[0].id, "d1");
        assert_eq!(identity.devices[0].name, "new phone");
        identity.pair("d3".into(), "", key(), t).unwrap();
        assert_eq!(identity.devices[1].name, "Unnamed device");
        assert!(identity.revoke("d1"));
        assert!(!identity.revoke("d1"));
        assert!(identity.device(&phone).is_none());
    }

    #[test]
    fn the_number_of_devices_is_bounded() {
        let mut identity = Identity {
            keypair: Keypair::generate().unwrap(),
            devices: Vec::new(),
        };
        let t = Timestamp::from_unix_millis(1);
        for i in 0..MAX_DEVICES {
            identity.pair(format!("d{i}"), "x", key(), t).unwrap();
        }
        assert!(identity.pair("extra".into(), "x", key(), t).is_err());
    }
}
