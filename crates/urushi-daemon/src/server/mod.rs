//! The protocol server: local IPC, tailnet and LAN listeners feed connections into
//! [`connection::serve`], which translates frames into calls on the service.

pub(crate) mod accept;
pub(crate) mod attach;
mod boards;
pub(crate) mod connection;
pub(crate) mod handlers;
mod jira;
mod jira_issue;
pub(crate) mod lan;
mod organize;
mod quick_filters;
pub(crate) mod remote;
pub(crate) mod screen;
mod usage;

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
