//! The bridge between the UI thread and the daemon.
//!
//! A tokio runtime on background threads owns the connection. It (re)connects, forwards
//! daemon events, and runs request effects. Everything it learns goes back to the UI
//! thread as [`Msg`]s through the winit event-loop proxy. The UI thread never blocks on
//! the network.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use urushi_client::{Client, LanKeyring, StopMethod, ensure_local_daemon, stop_local_daemon};
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;
use crate::config::ConnectTarget;
use crate::msg::{Effect, Msg, PairedHost};

const RETRY: Duration = Duration::from_secs(2);
/// How often [`Msg::Tick`] is sent.
const TICK: Duration = Duration::from_secs(3);
const CLIENT_NAME: &str = "urushi-app";

/// Owns the runtime and the current connection.
pub(crate) struct Backend {
    runtime: Runtime,
    proxy: EventLoopProxy<UserEvent>,
    client: Arc<Mutex<Option<Client>>>,
    connection: Option<JoinHandle<()>>,
    /// Where this device's LAN keyring lives (`None`: nowhere to keep one).
    keyring_dir: Option<PathBuf>,
}

impl Backend {
    /// Starts the runtime and connects to `target` (unless it is `Ask`). LAN hosts are
    /// paired in a keyring in `keyring_dir`.
    pub(crate) fn start(
        target: &ConnectTarget,
        proxy: EventLoopProxy<UserEvent>,
        keyring_dir: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("urushi-net")
            .build()?;
        runtime.spawn(ticks(proxy.clone()));
        let mut backend = Self {
            runtime,
            proxy,
            client: Arc::new(Mutex::new(None)),
            connection: None,
            keyring_dir,
        };
        backend.send_lan_hosts();
        backend.connect(target.clone());
        Ok(backend)
    }

    /// Drops any current connection and connects to `target`.
    pub(crate) fn connect(&mut self, target: ConnectTarget) {
        if let Some(task) = self.connection.take() {
            task.abort();
        }
        *self.client.lock().unwrap_or_else(PoisonError::into_inner) = None;
        if target == ConnectTarget::Ask {
            return;
        }
        let (proxy, slot) = (self.proxy.clone(), self.client.clone());
        let keyring_dir = self.keyring_dir.clone();
        self.connection =
            Some(
                self.runtime
                    .spawn(connection_loop(target, keyring_dir, proxy, slot)),
            );
    }

    /// Pairs with the LAN host at `host` using `code`, off the UI thread, then sends
    /// [`Msg::LanPaired`] and the new list of paired hosts.
    pub(crate) fn pair_lan(&self, host: String, code: String) {
        let proxy = self.proxy.clone();
        let dir = self.keyring_dir.clone();
        self.runtime.spawn(async move {
            let result = async {
                let dir = dir.ok_or("this device has nowhere to keep a LAN keyring")?;
                let mut keyring = LanKeyring::open(&dir).map_err(|e| e.to_string())?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
                let name = crate::platform::device_name();
                let paired = urushi_client::pair_lan(&host, &code, &name, &mut keyring, now)
                    .await
                    .map_err(|e| e.to_string())?;
                send(&proxy, Msg::LanHosts(paired_hosts(&keyring)));
                Ok::<_, String>(paired.address())
            }
            .await;
            send(&proxy, Msg::LanPaired(result));
        });
    }

    /// Sends [`Msg::LanHosts`] with the hosts in the keyring.
    fn send_lan_hosts(&self) {
        let Some(dir) = self.keyring_dir.clone() else {
            return;
        };
        let proxy = self.proxy.clone();
        self.runtime.spawn(async move {
            match LanKeyring::open(&dir) {
                Ok(keyring) => send(&proxy, Msg::LanHosts(paired_hosts(&keyring))),
                Err(e) => tracing::warn!(error = %e, "cannot read the LAN keyring"),
            }
        });
    }

    /// Drops the connection and stops the local daemon in `home`, then sends
    /// [`Msg::DaemonStopped`]. Reconnecting afterwards starts this app's daemon.
    pub(crate) fn stop_daemon(&mut self, home: Option<std::path::PathBuf>) {
        if let Some(task) = self.connection.take() {
            task.abort();
        }
        *self.client.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let proxy = self.proxy.clone();
        self.runtime.spawn(async move {
            let result = stop_local_daemon(home.as_deref(), StopMethod::Auto)
                .await
                .map(drop)
                .map_err(|e| e.to_string());
            send(&proxy, Msg::DaemonStopped(result));
        });
    }

    /// Runs a request or notification effect. Other effects are handled by the runner.
    pub(crate) fn execute(&self, effect: Effect) {
        let Some(client) = self
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            if let Effect::Request { followup, .. } = effect {
                send(
                    &self.proxy,
                    Msg::Response {
                        followup,
                        result: Err("not connected to the daemon".into()),
                    },
                );
            }
            return;
        };
        let proxy = self.proxy.clone();
        match effect {
            Effect::Request { body, followup } => {
                self.runtime.spawn(async move {
                    let result = client.request(body).await.map_err(|e| e.to_string());
                    send(&proxy, Msg::Response { followup, result });
                });
            }
            // Queued right here, on the UI thread, so input reaches the daemon in the
            // order it happened. (A task per notification could reorder a click's press
            // and release.)
            Effect::Notify(body) => {
                if let Err(e) = client.try_notify(body) {
                    tracing::warn!(error = %e, "dropped a notification");
                }
            }
            _ => {}
        }
    }
}

fn send(proxy: &EventLoopProxy<UserEvent>, msg: Msg) {
    // Fails only when the event loop has exited, at which point nothing matters.
    let _ = proxy.send_event(UserEvent::App(Box::new(msg)));
}

/// Sends [`Msg::Tick`] with the wall-clock time every few seconds, for as long as the
/// app runs. The status bar refreshes the agent usage on each one.
async fn ticks(proxy: EventLoopProxy<UserEvent>) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        send(&proxy, Msg::Tick(now));
    }
}

fn paired_hosts(keyring: &LanKeyring) -> Vec<PairedHost> {
    keyring
        .hosts()
        .iter()
        .map(|h| PairedHost {
            address: h.address(),
            host_name: h.host_name.clone(),
        })
        .collect()
}

async fn connect(
    target: &ConnectTarget,
    keyring_dir: Option<&Path>,
) -> Result<(Client, urushi_client::Events), String> {
    match target {
        ConnectTarget::Local(home) => ensure_local_daemon(home.as_deref(), CLIENT_NAME)
            .await
            .map_err(|e| e.to_string()),
        ConnectTarget::Remote(host) => {
            // Paired LAN hosts go through the encrypted channel; others are tailnet.
            let keyring = match keyring_dir.map(LanKeyring::open) {
                Some(Ok(keyring)) => Some(keyring),
                Some(Err(e)) => return Err(e.to_string()),
                None => None,
            };
            Client::connect_remote(host, keyring.as_ref(), CLIENT_NAME)
                .await
                .map_err(|e| e.to_string())
        }
        ConnectTarget::Ask => Err("no host chosen".into()),
    }
}

async fn connection_loop(
    target: ConnectTarget,
    keyring_dir: Option<PathBuf>,
    proxy: EventLoopProxy<UserEvent>,
    slot: Arc<Mutex<Option<Client>>>,
) {
    loop {
        match connect(&target, keyring_dir.as_deref()).await {
            Ok((client, mut events)) => match client.list_workspace(true).await {
                Ok(workspace) => {
                    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(client.clone());
                    send(
                        &proxy,
                        Msg::Connected {
                            welcome: client.welcome().clone(),
                            workspace,
                        },
                    );
                    while let Some(event) = events.recv().await {
                        send(&proxy, Msg::Daemon(event));
                    }
                    *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    send(
                        &proxy,
                        Msg::Disconnected(
                            "connection to the daemon was lost; reconnecting…".into(),
                        ),
                    );
                }
                Err(e) => send(&proxy, Msg::Disconnected(e.to_string())),
            },
            Err(e) => send(&proxy, Msg::Disconnected(e)),
        }
        tokio::time::sleep(RETRY).await;
    }
}
