//! The protocol server: local IPC and tailnet listeners feed connections into
//! [`connection::serve`], which translates frames into calls on the service.

pub(crate) mod attach;
pub(crate) mod connection;
pub(crate) mod handlers;
pub(crate) mod remote;
pub(crate) mod screen;

use std::sync::OnceLock;

/// This machine's host name, reported in `Welcome`.
pub(crate) fn host_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "localhost".into())
    })
    .clone()
}
