//! `CliTailscale` against fake `tailscale` binaries.
//!
//! The macOS Tailscale.app binary is both the GUI and the CLI. Without a terminal (as
//! when Urushi.app starts the daemon) it tries to launch the GUI and prints an error to
//! stdout with exit code 0, unless `TAILSCALE_BE_CLI=1` is set. These fakes mimic that.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::fs;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use urushi_daemon::tailscale::{CliTailscale, TailscaleError, TailscaleProvider};

const GUI_ERROR: &str = "The Tailscale GUI failed to start: The operation couldn't be completed. (Tailscale.CLIError error 3.)";

const STATUS: &str = r#"{"BackendState": "Running", "Self": {"HostName": "mac", "DNSName": "mac.tail1.ts.net.", "TailscaleIPs": ["100.1.2.3"], "UserID": 42, "Online": true, "OS": "macOS"}, "User": {"42": {"LoginName": "me@example.com"}}}"#;

const WHOIS: &str = r#"{"Node": {"Name": "phone.tail1.ts.net.", "Tags": null}, "UserProfile": {"LoginName": "me@example.com"}}"#;

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("Tailscale");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Behaves like the macOS app binary started without a terminal.
fn app_like(dir: &Path) -> PathBuf {
    script(
        dir,
        &format!(
            r#"if [ "$TAILSCALE_BE_CLI" != "1" ]; then echo "{GUI_ERROR}"; exit 0; fi
case "$1" in
  status) echo '{STATUS}' ;;
  whois) echo '{WHOIS}' ;;
esac"#
        ),
    )
}

#[test]
fn status_works_with_the_macos_app_binary_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let ts = CliTailscale::with_binary(app_like(dir.path()));
    let status = ts.status().expect("status");
    assert_eq!(status.dns_name, "mac.tail1.ts.net");
    assert_eq!(status.login, "me@example.com");
    assert_eq!(
        status.addresses,
        vec!["100.1.2.3".parse::<IpAddr>().unwrap()]
    );
}

#[test]
fn whois_works_with_the_macos_app_binary_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let ts = CliTailscale::with_binary(app_like(dir.path()));
    let identity = ts.whois("100.9.9.9".parse().unwrap()).expect("whois");
    assert_eq!(identity.login, "me@example.com");
    assert_eq!(identity.node, "phone.tail1.ts.net");
}

#[test]
fn non_json_output_is_reported_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let ts = CliTailscale::with_binary(script(dir.path(), &format!("echo \"{GUI_ERROR}\"")));
    let err = ts.status().unwrap_err();
    let TailscaleError::Cli(message) = err else {
        panic!("expected a CLI error, got {err:?}");
    };
    assert!(
        message.contains("The Tailscale GUI failed to start"),
        "message should quote the output: {message}"
    );
}

#[test]
fn missing_binary_is_a_cli_error() {
    let ts = CliTailscale::with_binary("/nonexistent/tailscale");
    assert!(matches!(ts.status(), Err(TailscaleError::Cli(_))));
}
