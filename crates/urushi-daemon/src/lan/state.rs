//! What the daemon keeps while LAN access is on: its key and paired devices, the active
//! pairing code, the listeners, and which devices are connected.
//!
//! Each live connection of a paired device holds the receiving end of a `watch` channel
//! whose sender lives here. Dropping the sender (revoking the device, or turning LAN
//! access off) ends the connection.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use urushi_core::Timestamp;
use urushi_proto::lan::{Keypair, PairedDevice, PairingCode, PublicKey};
use urushi_proto::v1;

use super::identity::{Identity, IdentityError, IdentityStore};
use super::pairing::Pairing;

/// Handshakes that may be in progress at once. More connections wait for none: they are
/// dropped, so a flood can't tie the daemon up.
pub(crate) const MAX_PENDING_HANDSHAKES: usize = 8;

/// A paired device's connection, as registered by [`LanState::connected`].
pub(crate) struct LiveConnection {
    pub(crate) device_id: String,
    pub(crate) device_name: String,
    pub(crate) id: u64,
    /// Changes (or closes) when the connection must end.
    pub(crate) close: watch::Receiver<bool>,
}

/// LAN access state, owned by the service.
pub(crate) struct LanState {
    store: IdentityStore,
    identity: Option<Identity>,
    pairing: Option<Pairing>,
    pub(crate) listeners: Vec<(SocketAddr, JoinHandle<()>)>,
    pub(crate) error: Option<String>,
    pub(crate) handshakes: Arc<Semaphore>,
    connections: HashMap<String, Vec<(u64, watch::Sender<bool>)>>,
    next_connection: u64,
}

impl LanState {
    /// State for the daemon at `home`. Nothing is read or written until LAN access is
    /// used.
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            store: IdentityStore::in_home(home),
            identity: None,
            pairing: None,
            listeners: Vec::new(),
            error: None,
            handshakes: Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES)),
            connections: HashMap::new(),
            next_connection: 0,
        }
    }

    /// The identity, loaded (or created) on first use.
    pub(crate) fn identity(&mut self) -> Result<&mut Identity, IdentityError> {
        if self.identity.is_none() {
            self.identity = Some(self.store.load_or_create()?);
        }
        self.identity
            .as_mut()
            .ok_or_else(|| IdentityError::Damaged("not loaded".into()))
    }

    /// The daemon's keypair.
    pub(crate) fn keypair(&mut self) -> Result<Keypair, IdentityError> {
        Ok(self.identity()?.keypair.clone())
    }

    /// Whether `key` belongs to a paired device.
    pub(crate) fn is_paired(&self, key: &PublicKey) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|identity| identity.device(key).is_some())
    }

    /// Starts pairing with a new code, replacing any active one.
    pub(crate) fn start_pairing(&mut self, now: Instant) -> Result<PairingCode, String> {
        self.identity().map_err(|e| e.to_string())?;
        let code = PairingCode::generate().map_err(|e| e.to_string())?;
        self.pairing = Some(Pairing::new(code, now));
        Ok(code)
    }

    /// Withdraws the active code, if any.
    pub(crate) fn cancel_pairing(&mut self) {
        self.pairing = None;
    }

    /// The active code, if one still works.
    pub(crate) fn pairing_code(&mut self, now: Instant) -> Option<PairingCode> {
        let code = self.pairing.as_ref().and_then(|p| p.code_at(now));
        if code.is_none() {
            self.pairing = None;
        }
        code
    }

    /// Records a wrong pairing attempt.
    pub(crate) fn pairing_failed(&mut self) {
        if let Some(pairing) = &mut self.pairing {
            pairing.failed();
        }
    }

    /// Pairs `device`, using up the active code (which must be `code`), and saves.
    pub(crate) fn register(
        &mut self,
        device: &PairedDevice,
        code: PairingCode,
        id: String,
        now: Timestamp,
        at: Instant,
    ) -> Result<(), String> {
        if self.pairing_code(at) != Some(code) {
            return Err("the pairing code was used or withdrawn".into());
        }
        self.pairing = None;
        let identity = self.identity().map_err(|e| e.to_string())?;
        identity.pair(id, &device.name, device.key, now)?;
        let snapshot = identity.clone();
        self.store.save(&snapshot).map_err(|e| e.to_string())?;
        tracing::info!(device = %device.name, key = ?device.key, "paired a LAN device");
        Ok(())
    }

    /// Registers a connection of the paired device with `key`, and notes when it was
    /// seen. `None` if it isn't paired (any more).
    pub(crate) fn connected(&mut self, key: &PublicKey, now: Timestamp) -> Option<LiveConnection> {
        let identity = self.identity.as_mut()?;
        let device = identity.devices.iter_mut().find(|d| d.key == *key)?;
        device.last_seen = Some(now);
        let (device_id, device_name) = (device.id.clone(), device.name.clone());
        let snapshot = identity.clone();
        if let Err(e) = self.store.save(&snapshot) {
            tracing::warn!(error = %e, "cannot note when a LAN device was seen");
        }
        self.next_connection += 1;
        let id = self.next_connection;
        let (sender, close) = watch::channel(false);
        self.connections
            .entry(device_id.clone())
            .or_default()
            .push((id, sender));
        Some(LiveConnection {
            device_id,
            device_name,
            id,
            close,
        })
    }

    /// Forgets a connection that ended.
    pub(crate) fn disconnected(&mut self, device_id: &str, id: u64) {
        if let Some(list) = self.connections.get_mut(device_id) {
            list.retain(|(c, _)| *c != id);
            if list.is_empty() {
                self.connections.remove(device_id);
            }
        }
    }

    /// Forgets a device and ends its connections. Returns whether it was paired.
    pub(crate) fn revoke(&mut self, device_id: &str) -> Result<bool, IdentityError> {
        let identity = self.identity()?;
        let revoked = identity.revoke(device_id);
        if revoked {
            let snapshot = identity.clone();
            self.store.save(&snapshot)?;
            tracing::info!(device = device_id, "revoked a LAN device");
        }
        self.connections.remove(device_id);
        Ok(revoked)
    }

    /// Stops listening (addresses changed, or LAN access is going off).
    pub(crate) fn stop_listeners(&mut self) {
        for (_, task) in self.listeners.drain(..) {
            task.abort();
        }
    }

    /// Turns LAN access off: no listeners, no pairing, every LAN connection ended.
    pub(crate) fn stop(&mut self) {
        self.stop_listeners();
        self.pairing = None;
        self.connections.clear();
        self.error = None;
    }

    /// The status for `LanStatus`. The pairing code goes only to clients on this
    /// machine (`show_code`).
    pub(crate) fn status(
        &self,
        enabled: bool,
        port: u16,
        show_code: bool,
        now: Instant,
    ) -> v1::LanStatus {
        let mut addresses: Vec<String> = self
            .listeners
            .iter()
            .map(|(addr, _)| addr.ip().to_string())
            .collect();
        addresses.dedup();
        let pairing = self
            .pairing
            .as_ref()
            .filter(|p| show_code && p.code_at(now).is_some())
            .map(|p| v1::LanPairing {
                code: p.code_at(now).map(|c| c.to_string()).unwrap_or_default(),
                expires_in_secs: u32::try_from(p.remaining_at(now).as_secs()).unwrap_or(u32::MAX),
            });
        let devices = self
            .identity
            .as_ref()
            .map(|identity| {
                identity
                    .devices
                    .iter()
                    .map(|d| v1::LanDevice {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        fingerprint: d.key.fingerprint(),
                        paired_at_ms: millis(d.paired_at),
                        last_seen_ms: d.last_seen.map_or(0, millis),
                        connected: self.connections.contains_key(&d.id),
                    })
                    .collect()
            })
            .unwrap_or_default();
        v1::LanStatus {
            enabled,
            serving: !self.listeners.is_empty(),
            addresses,
            port: u32::from(port),
            error: self.error.clone().unwrap_or_default(),
            devices,
            pairing,
            fingerprint: self
                .identity
                .as_ref()
                .map(|i| i.keypair.public().fingerprint())
                .unwrap_or_default(),
        }
    }
}

fn millis(t: Timestamp) -> i64 {
    i64::try_from(t.unix_millis()).unwrap_or(i64::MAX)
}
