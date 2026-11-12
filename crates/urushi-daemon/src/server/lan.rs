//! Serving the daemon to paired devices on the LAN (ADR 0041).
//!
//! With `lan.enabled` on, the daemon listens on each of this machine's private addresses
//! (never on every interface, never on a public address) and re-checks them every 30 s.
//! It never asks a router or firewall to open anything.
//!
//! Every connection must come from a private or loopback address, then complete a Noise
//! handshake within [`HANDSHAKE_TIMEOUT`], with at most a few handshakes at once:
//! - **connect**: only keys of paired devices are answered; the protocol then runs
//!   inside the encrypted stream, as a remote (non-local) peer;
//! - **pair**: only while a pairing code is active; the device is recorded and the
//!   connection ends.

use std::net::{IpAddr, SocketAddr};
use std::ops::ControlFlow;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use urushi_proto::lan::{self, LanError, Mode};
use urushi_proto::v1;

use super::accept::accept_forever;
use super::connection::{self, Peer};
use crate::lan::addresses::is_allowed_peer;
use crate::service::{Service, ServiceError, ServiceResult, lock, new_uuid, now};

const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);
/// Longest a device may take to finish its handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Makes the listeners match the settings and the current private addresses.
pub(crate) async fn reconcile(service: &Arc<Service>) {
    let settings = service.settings().lan;
    if !settings.enabled {
        lock(&service.lan).stop();
        return;
    }
    // The key is made the first time LAN access is turned on.
    if let Err(e) = lock(&service.lan).identity() {
        fail(service, e.to_string());
        return;
    }
    let interfaces = service.lan_interfaces.clone();
    let addresses = match tokio::task::spawn_blocking(move || interfaces.addresses()).await {
        Ok(Ok(addresses)) => addresses,
        Ok(Err(e)) => return fail(service, format!("cannot list network addresses: {e}")),
        Err(e) => return fail(service, e.to_string()),
    };
    let desired: Vec<SocketAddr> = addresses
        .into_iter()
        .filter(|ip| listenable(*ip))
        .map(|ip| SocketAddr::new(ip, settings.port))
        .collect();
    if desired.is_empty() {
        return fail(service, "this machine has no private LAN address".into());
    }
    {
        let mut state = lock(&service.lan);
        let current: Vec<SocketAddr> = state.listeners.iter().map(|(a, _)| *a).collect();
        if current == desired {
            return;
        }
        state.stop_listeners();
    }

    let mut listeners = Vec::new();
    let mut errors = Vec::new();
    for addr in desired {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                tracing::info!(%addr, "serving on the LAN");
                listeners.push((
                    addr,
                    tokio::spawn(accept_loop(Arc::downgrade(service), listener)),
                ));
            }
            Err(e) => errors.push(format!("cannot listen on {addr}: {e}")),
        }
    }
    let mut state = lock(&service.lan);
    state.stop_listeners();
    state.listeners = listeners;
    state.error = (!errors.is_empty()).then(|| errors.join("; "));
}

/// Only a specific private (or, for tests, loopback) address: never `0.0.0.0` or `::`.
fn listenable(ip: IpAddr) -> bool {
    !ip.is_unspecified() && is_allowed_peer(ip)
}

fn fail(service: &Arc<Service>, error: String) {
    let mut state = lock(&service.lan);
    if state.error.as_deref() != Some(error.as_str()) {
        tracing::warn!(%error, "cannot serve on the LAN");
    }
    state.stop_listeners();
    state.error = Some(error);
}

/// Periodically re-checks the addresses (they change with DHCP or another network).
pub(crate) fn spawn_watchdog(service: Weak<Service>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(WATCHDOG_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(service) = service.upgrade() else {
                return;
            };
            if service.settings().lan.enabled {
                reconcile(&service).await;
            }
        }
    })
}

async fn accept_loop(service: Weak<Service>, listener: TcpListener) {
    accept_forever(
        "LAN",
        || listener.accept(),
        |(stream, peer)| {
            if !is_allowed_peer(peer.ip()) {
                tracing::warn!(%peer, "refused a LAN connection from outside the LAN");
                return ControlFlow::Continue(());
            }
            let Some(service) = service.upgrade() else {
                return ControlFlow::Break(());
            };
            let _ = stream.set_nodelay(true);
            let handshakes = lock(&service.lan).handshakes.clone();
            let Ok(permit) = handshakes.try_acquire_owned() else {
                tracing::warn!(%peer, "refused a LAN connection: too many handshakes at once");
                return ControlFlow::Continue(());
            };
            tokio::spawn(async move {
                let handshake =
                    tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake(&service, stream, peer))
                        .await;
                drop(permit);
                match handshake {
                    Ok(Ok(Some((stream, live)))) => {
                        let description = format!("LAN device {} at {peer}", live.device_name);
                        connection::serve_boxed(
                            service.clone(),
                            stream,
                            Peer {
                                description,
                                local: false,
                                close: Some(live.close),
                            },
                        )
                        .await;
                        lock(&service.lan).disconnected(&live.device_id, live.id);
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => tracing::warn!(%peer, error = %e, "refused a LAN connection"),
                    Err(_) => {
                        tracing::warn!(%peer, "refused a LAN connection: handshake timed out");
                    }
                }
            });
            ControlFlow::Continue(())
        },
    )
    .await;
}

type Accepted = (
    lan::SecureStream<TcpStream>,
    crate::lan::state::LiveConnection,
);

/// Runs the handshake. A paired device gets its encrypted stream; a pairing ends here.
async fn handshake(
    service: &Arc<Service>,
    mut stream: TcpStream,
    peer: SocketAddr,
) -> Result<Option<Accepted>, LanError> {
    let mode = lan::read_prelude(&mut stream).await?;
    let keypair = lock(&service.lan)
        .keypair()
        .map_err(|_| LanError::BadMessage("the daemon has no LAN key"))?;
    match mode {
        Mode::Connect => {
            let (secure, key) =
                lan::accept_connect(stream, &keypair, |key| lock(&service.lan).is_paired(key))
                    .await?;
            let live = lock(&service.lan)
                .connected(&key, now())
                .ok_or(LanError::Refused)?;
            tracing::info!(%peer, device = %live.device_name, "LAN device connected");
            Ok(Some((secure, live)))
        }
        Mode::Pair => {
            let code = lock(&service.lan)
                .pairing_code(Instant::now())
                .ok_or(LanError::PairingRefused)?;
            let result = lan::accept_pair(stream, &keypair, code, &super::host_name(), |device| {
                lock(&service.lan)
                    .register(device, code, new_uuid().to_string(), now(), Instant::now())
                    .map_err(|reason| {
                        tracing::warn!(%reason, "cannot pair a LAN device");
                        LanError::PairingRefused
                    })
            })
            .await;
            if let Err(LanError::PairingRefused) = &result {
                lock(&service.lan).pairing_failed();
            }
            result.map(|_| None)
        }
    }
}

/// The status for `LanStatus`. Only clients on this machine see the pairing code.
pub(crate) fn status(service: &Arc<Service>, local: bool) -> v1::LanStatus {
    let settings = service.settings().lan;
    lock(&service.lan).status(settings.enabled, settings.port, local, Instant::now())
}

/// Starts pairing a device. LAN access must be on.
pub(crate) fn start_pairing(service: &Arc<Service>) -> ServiceResult<()> {
    if !service.settings().lan.enabled {
        return Err(ServiceError::Invalid("turn on LAN access first".into()));
    }
    lock(&service.lan)
        .start_pairing(Instant::now())
        .map(|_| ())
        .map_err(ServiceError::Internal)
}

/// Withdraws the pairing code.
pub(crate) fn cancel_pairing(service: &Arc<Service>) {
    lock(&service.lan).cancel_pairing();
}

/// Forgets a paired device and ends its connections.
pub(crate) fn revoke(service: &Arc<Service>, device_id: &str) -> ServiceResult<()> {
    match lock(&service.lan).revoke(device_id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ServiceError::NotFound(format!("LAN device {device_id}"))),
        Err(e) => Err(ServiceError::Internal(e.to_string())),
    }
}
