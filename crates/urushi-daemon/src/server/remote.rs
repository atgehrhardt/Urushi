//! Serving the daemon on the user's tailnet (ADR 0005).
//!
//! When `tailnet.serve` is on, the daemon listens on each of this machine's tailnet
//! addresses, never on all interfaces. Every incoming connection is identified with
//! `tailscale whois` and checked by [`authorize`] before the protocol handshake. A
//! watchdog re-checks the addresses periodically and re-binds when they change.
//!
//! Every admitted connection holds the receiving end of a `watch` channel whose sender
//! lives in [`RemoteState`], next to the identity it was admitted as. Each reconcile
//! checks those identities again and drops the senders of the ones no longer allowed,
//! so turning serving off or narrowing the allowlist ends live sessions too (ADR 0061).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::ops::ControlFlow;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use urushi_core::TailnetSettings;
use urushi_proto::v1;

use super::accept::accept_forever;
use super::connection::{self, Peer};
use crate::service::{Service, lock};
use crate::tailscale::{Identity, TailscaleError};

const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);

/// Connections that may be waiting for `tailscale whois` at once. More are dropped, so
/// a flood from the tailnet can't pile up processes and blocking threads.
pub(crate) const MAX_PENDING_IDENTIFICATIONS: usize = 8;

/// Listener state and live connections, owned by the service.
pub(crate) struct RemoteState {
    listeners: Vec<(SocketAddr, JoinHandle<()>)>,
    error: Option<String>,
    owner_login: Option<String>,
    identifying: Arc<Semaphore>,
    /// Admitted connections by id: who they are, and the sender that ends them when
    /// dropped.
    connections: HashMap<u64, (Identity, watch::Sender<bool>)>,
    next_connection: u64,
}

impl Default for RemoteState {
    fn default() -> Self {
        Self {
            listeners: Vec::new(),
            error: None,
            owner_login: None,
            identifying: Arc::new(Semaphore::new(MAX_PENDING_IDENTIFICATIONS)),
            connections: HashMap::new(),
            next_connection: 0,
        }
    }
}

impl RemoteState {
    /// Stops listening. Live connections stay; see [`Self::end_all`].
    fn stop(&mut self) {
        for (_, task) in self.listeners.drain(..) {
            task.abort();
        }
    }

    /// Ends every tailnet connection (serving turned off).
    fn end_all(&mut self) {
        self.connections.clear();
    }

    /// Ends the connections whose identity `settings` no longer admit.
    fn recheck(&mut self, settings: &TailnetSettings) {
        let owner = self.owner_login.clone();
        self.connections.retain(|_, (identity, _)| {
            let allowed = authorize(identity, settings, owner.as_deref());
            if let Err(reason) = &allowed {
                tracing::info!(%reason, "ending a tailnet connection whose access was withdrawn");
            }
            allowed.is_ok()
        });
    }

    /// Admits `identity` if `settings` allow it, registering the connection. Checked
    /// here, under the state's lock, so a settings change can't slip between the check
    /// and the registration.
    fn admit(
        &mut self,
        identity: &Identity,
        settings: &TailnetSettings,
    ) -> Result<(u64, watch::Receiver<bool>), String> {
        if !settings.serve {
            return Err("tailnet serving is off".into());
        }
        authorize(identity, settings, self.owner_login.as_deref())?;
        self.next_connection += 1;
        let id = self.next_connection;
        let (sender, close) = watch::channel(false);
        self.connections.insert(id, (identity.clone(), sender));
        Ok((id, close))
    }
}

/// Decides whether a tailnet identity may connect.
///
/// - Tagged devices (owned by ACL tags, not people) are denied unless `allow_tagged_nodes`.
/// - With an explicit allowlist, the login must be on it.
/// - Otherwise only the owner of this machine may connect.
pub(crate) fn authorize(
    identity: &Identity,
    settings: &TailnetSettings,
    owner_login: Option<&str>,
) -> Result<(), String> {
    if !identity.tags.is_empty() && !settings.allow_tagged_nodes {
        return Err(format!("tagged device {} is not allowed", identity.node));
    }
    if identity.tags.is_empty() && identity.login.is_empty() {
        return Err("device has no identifiable owner".into());
    }
    let allowed = if settings.allowed_logins.is_empty() {
        owner_login.is_some_and(|owner| owner.eq_ignore_ascii_case(&identity.login))
    } else {
        settings
            .allowed_logins
            .iter()
            .any(|l| l.eq_ignore_ascii_case(&identity.login))
    };
    if allowed || !identity.tags.is_empty() {
        Ok(())
    } else {
        Err(format!("{} is not allowed to connect", identity.login))
    }
}

/// Makes the listeners match the settings and the current tailnet addresses.
pub(crate) async fn reconcile(service: &Arc<Service>) {
    let settings = service.settings().tailnet;
    if !settings.serve {
        let mut state = lock(&service.remote);
        state.stop();
        state.end_all();
        state.error = None;
        return;
    }
    let tailscale = service.tailscale.clone();
    let status = match tokio::task::spawn_blocking(move || tailscale.status()).await {
        Ok(result) => result,
        Err(e) => Err(TailscaleError::Cli(e.to_string())),
    };
    let status = match status {
        Ok(status) => status,
        Err(e) => {
            let error = e.to_string();
            let mut state = lock(&service.remote);
            if state.error.as_deref() != Some(error.as_str()) {
                tracing::warn!(%error, "cannot serve on tailnet");
            }
            state.stop();
            state.error = Some(error);
            return;
        }
    };
    let desired: Vec<SocketAddr> = status
        .addresses
        .iter()
        .map(|ip| SocketAddr::new(*ip, settings.port))
        .collect();
    {
        let mut state = lock(&service.remote);
        state.owner_login = Some(status.login.clone()).filter(|l| !l.is_empty());
        state.recheck(&settings);
        let current: Vec<SocketAddr> = state.listeners.iter().map(|(a, _)| *a).collect();
        if current == desired && !current.is_empty() {
            return;
        }
        state.stop();
    }

    let mut listeners = Vec::new();
    let mut errors = Vec::new();
    for addr in desired {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                tracing::info!(%addr, "serving on tailnet");
                listeners.push((
                    addr,
                    tokio::spawn(accept_loop(Arc::downgrade(service), listener)),
                ));
            }
            Err(e) => errors.push(format!("cannot listen on {addr}: {e}")),
        }
    }
    let mut state = lock(&service.remote);
    state.stop();
    state.listeners = listeners;
    state.error = (!errors.is_empty()).then(|| errors.join("; "));
}

/// Periodically re-checks tailnet addresses (they can change, e.g. after re-login).
pub(crate) fn spawn_watchdog(service: Weak<Service>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(WATCHDOG_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(service) = service.upgrade() else {
                return;
            };
            if service.settings().tailnet.serve {
                reconcile(&service).await;
            }
        }
    })
}

async fn accept_loop(service: Weak<Service>, listener: TcpListener) {
    accept_forever(
        "tailnet",
        || listener.accept(),
        |(stream, peer)| {
            let Some(service) = service.upgrade() else {
                return ControlFlow::Break(());
            };
            let identifying = lock(&service.remote).identifying.clone();
            let Ok(permit) = identifying.try_acquire_owned() else {
                tracing::warn!(%peer, "refused a tailnet connection: too many being identified");
                return ControlFlow::Continue(());
            };
            tokio::spawn(async move {
                let admitted = identify(&service, peer.ip()).await;
                drop(permit);
                match admitted {
                    Ok((identity, id, close)) => {
                        let description =
                            format!("{} ({}) at {peer}", identity.login, identity.node);
                        let _ = stream.set_nodelay(true);
                        connection::serve_boxed(
                            service.clone(),
                            stream,
                            Peer {
                                description,
                                local: false,
                                close: Some(close),
                            },
                        )
                        .await;
                        lock(&service.remote).connections.remove(&id);
                    }
                    Err(reason) => tracing::warn!(%peer, %reason, "refused tailnet connection"),
                }
            });
            ControlFlow::Continue(())
        },
    )
    .await;
}

/// Identifies the peer at `ip` and admits it, returning its identity, its connection
/// id and the receiver that ends it.
async fn identify(
    service: &Arc<Service>,
    ip: IpAddr,
) -> Result<(Identity, u64, watch::Receiver<bool>), String> {
    let tailscale = service.tailscale.clone();
    let identity = tokio::task::spawn_blocking(move || tailscale.whois(ip))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let mut state = lock(&service.remote);
    // Read under the lock: a reconcile after a settings change rechecks under it too,
    // so this connection is either admitted by the new settings or ended by it.
    let settings = service.settings().tailnet;
    let (id, close) = state.admit(&identity, &settings)?;
    Ok((identity, id, close))
}

/// Current tailnet status for the `TailnetStatus` request.
pub(crate) async fn status(service: &Arc<Service>) -> v1::TailnetStatus {
    let settings = service.settings().tailnet;
    let tailscale = service.tailscale.clone();
    let result = tokio::task::spawn_blocking(move || tailscale.status())
        .await
        .unwrap_or_else(|e| Err(TailscaleError::Cli(e.to_string())));
    let (serving, listen_error) = {
        let state = lock(&service.remote);
        (!state.listeners.is_empty(), state.error.clone())
    };
    let port = u32::from(settings.port);
    match result {
        Ok(s) => v1::TailnetStatus {
            installed: true,
            running: true,
            dns_name: s.dns_name,
            addresses: s.addresses.iter().map(ToString::to_string).collect(),
            login: s.login,
            serving,
            port,
            error: listen_error.unwrap_or_default(),
            peers: s
                .peers
                .into_iter()
                .map(|p| v1::TailnetPeer {
                    name: p.name,
                    dns_name: p.dns_name,
                    addresses: p.addresses.iter().map(ToString::to_string).collect(),
                    online: p.online,
                    os: p.os,
                })
                .collect(),
        },
        Err(e) => v1::TailnetStatus {
            installed: e != TailscaleError::NotInstalled,
            running: false,
            serving,
            port,
            error: e.to_string(),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(login: &str, tags: &[&str]) -> Identity {
        Identity {
            login: login.into(),
            node: "dev".into(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn default_allows_only_the_owner() {
        let s = TailnetSettings::default();
        assert!(authorize(&id("me@x.com", &[]), &s, Some("me@x.com")).is_ok());
        assert!(authorize(&id("ME@x.com", &[]), &s, Some("me@x.com")).is_ok());
        assert!(authorize(&id("other@x.com", &[]), &s, Some("me@x.com")).is_err());
        assert!(
            authorize(&id("me@x.com", &[]), &s, None).is_err(),
            "unknown owner denies everyone"
        );
    }

    #[test]
    fn allowlist_replaces_owner_rule() {
        let s = TailnetSettings {
            allowed_logins: vec!["friend@x.com".into()],
            ..TailnetSettings::default()
        };
        assert!(authorize(&id("friend@x.com", &[]), &s, Some("me@x.com")).is_ok());
        assert!(authorize(&id("me@x.com", &[]), &s, Some("me@x.com")).is_err());
    }

    #[test]
    fn rechecks_end_only_connections_the_settings_no_longer_admit() {
        let mut state = RemoteState {
            owner_login: Some("me@x.com".into()),
            ..RemoteState::default()
        };
        let mut s = TailnetSettings {
            serve: true,
            ..TailnetSettings::default()
        };
        let (_, mut me) = state.admit(&id("me@x.com", &[]), &s).unwrap();
        s.allow_tagged_nodes = true;
        let (_, ci) = state.admit(&id("tagged-devices", &["tag:ci"]), &s).unwrap();
        assert!(state.admit(&id("other@x.com", &[]), &s).is_err());

        s.allow_tagged_nodes = false;
        state.recheck(&s);
        assert!(
            ci.has_changed().is_err(),
            "the tagged device's sender is gone"
        );
        assert!(me.has_changed().is_ok(), "the owner stays");
        assert!(me.borrow_and_update().eq(&false));

        state.end_all();
        assert!(me.has_changed().is_err());
        s.serve = false;
        assert!(
            state.admit(&id("me@x.com", &[]), &s).is_err(),
            "not while off"
        );
    }

    #[test]
    fn tagged_devices_need_opt_in() {
        let mut s = TailnetSettings::default();
        assert!(authorize(&id("tagged-devices", &["tag:ci"]), &s, Some("me@x.com")).is_err());
        s.allow_tagged_nodes = true;
        assert!(authorize(&id("tagged-devices", &["tag:ci"]), &s, Some("me@x.com")).is_ok());
        assert!(authorize(&id("", &[]), &s, Some("me@x.com")).is_err());
    }
}
