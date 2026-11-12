//! Serving the daemon on the user's tailnet (ADR 0005).
//!
//! When `tailnet.serve` is on, the daemon listens on each of this machine's tailnet
//! addresses, never on all interfaces. Every incoming connection is identified with
//! `tailscale whois` and checked by [`authorize`] before the protocol handshake. A
//! watchdog re-checks the addresses periodically and re-binds when they change.

use std::net::{IpAddr, SocketAddr};
use std::ops::ControlFlow;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use urushi_core::TailnetSettings;
use urushi_proto::v1;

use super::accept::accept_forever;
use super::connection::{self, Peer};
use crate::service::{Service, lock};
use crate::tailscale::{Identity, TailscaleError};

const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);

/// Listener state, owned by the service.
#[derive(Default)]
pub(crate) struct RemoteState {
    listeners: Vec<(SocketAddr, JoinHandle<()>)>,
    error: Option<String>,
    owner_login: Option<String>,
}

impl RemoteState {
    fn stop(&mut self) {
        for (_, task) in self.listeners.drain(..) {
            task.abort();
        }
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
            tokio::spawn(async move {
                match identify(&service, peer.ip()).await {
                    Ok(identity) => {
                        let description =
                            format!("{} ({}) at {peer}", identity.login, identity.node);
                        let _ = stream.set_nodelay(true);
                        connection::serve_boxed(
                            service,
                            stream,
                            Peer {
                                description,
                                local: false,
                                close: None,
                            },
                        )
                        .await;
                    }
                    Err(reason) => tracing::warn!(%peer, %reason, "refused tailnet connection"),
                }
            });
            ControlFlow::Continue(())
        },
    )
    .await;
}

async fn identify(service: &Arc<Service>, ip: IpAddr) -> Result<Identity, String> {
    let tailscale = service.tailscale.clone();
    let identity = tokio::task::spawn_blocking(move || tailscale.whois(ip))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let settings = service.settings().tailnet;
    let owner = lock(&service.remote).owner_login.clone();
    authorize(&identity, &settings, owner.as_deref())?;
    Ok(identity)
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
    fn tagged_devices_need_opt_in() {
        let mut s = TailnetSettings::default();
        assert!(authorize(&id("tagged-devices", &["tag:ci"]), &s, Some("me@x.com")).is_err());
        s.allow_tagged_nodes = true;
        assert!(authorize(&id("tagged-devices", &["tag:ci"]), &s, Some("me@x.com")).is_ok());
        assert!(authorize(&id("", &[]), &s, Some("me@x.com")).is_err());
    }
}
