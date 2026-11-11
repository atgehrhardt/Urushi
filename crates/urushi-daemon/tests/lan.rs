//! LAN access end to end (ADR 0040): pairing, encrypted connections, revoking, and
//! what only the host may do. The daemon "listens on the LAN" on loopback here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Daemon, TIMEOUT, temp_home};
use urushi_client::{Client, ClientError, LanHost, LanKeyring, pair_lan, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::lan::StaticInterfaces;
use urushi_daemon::tailscale::StaticTailscale;
use urushi_proto::lan::LanError;

async fn start(home: &std::path::Path) -> Daemon {
    let mut options = DaemonOptions::new(home);
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.lan_interfaces = Arc::new(StaticInterfaces(vec![[127, 0, 0, 1].into()]));
    Daemon::start_options(options).await
}

async fn enable(client: &Client, port: u16) -> v1::LanStatus {
    client
        .update_settings(v1::SettingsPatch {
            lan_port: Some(port.into()),
            lan_enabled: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    client.lan_status().await.unwrap()
}

async fn new_code(client: &Client) -> String {
    client
        .start_lan_pairing()
        .await
        .unwrap()
        .pairing
        .unwrap()
        .code
}

/// Waits until `client`'s connection has closed.
async fn closes(client: &Client) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while client.list_workspace(false).await.is_ok() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the connection stayed open"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lan_access_is_off_until_switched_on() {
    let home = temp_home();
    let daemon = start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let status = client.lan_status().await.unwrap();
    assert!(!status.enabled && !status.serving && status.addresses.is_empty());
    assert!(
        !home.path().join("home/lan.json").exists(),
        "no key until it is used"
    );
    let err = client.start_lan_pairing().await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::InvalidArgument));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paired_device_connects_and_a_revoked_one_is_cut_off() {
    let home = temp_home();
    let daemon = start(&home.path().join("home")).await;
    let (host, _events) = daemon.connect().await;
    let port = 47_501;
    let status = enable(&host, port).await;
    assert!(status.enabled && status.serving, "{status:?}");
    assert_eq!(status.addresses, vec!["127.0.0.1"]);
    assert!(!status.fingerprint.is_empty());
    let spec = format!("127.0.0.1:{port}");

    // Without an active code, pairing is refused.
    let phone_dir = tempfile::tempdir().unwrap();
    let mut phone = LanKeyring::open(phone_dir.path()).unwrap();
    let err = pair_lan(&spec, "00000-00000-00000-00000", "phone", &mut phone, 1)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::Lan(LanError::PairingRefused)),
        "{err:?}"
    );

    // A wrong code is refused, and counts against the real one.
    let code = new_code(&host).await;
    let err = pair_lan(&spec, "00000-00000-00000-00000", "phone", &mut phone, 1)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::Lan(LanError::PairingRefused)),
        "{err:?}"
    );

    let paired = pair_lan(&spec, &code, "phone", &mut phone, 1)
        .await
        .unwrap();
    assert_eq!(paired.port, port);
    assert!(phone.find("127.0.0.1").is_some());
    let status = host.lan_status().await.unwrap();
    assert!(status.pairing.is_none(), "a code works once");
    assert_eq!(status.devices.len(), 1);
    assert_eq!(status.devices[0].name, "phone");
    assert_eq!(
        status.devices[0].fingerprint,
        phone.public_key().fingerprint()
    );

    // The same code can't pair a second device.
    let tablet_dir = tempfile::tempdir().unwrap();
    let mut tablet = LanKeyring::open(tablet_dir.path()).unwrap();
    assert!(
        pair_lan(&spec, &code, "tablet", &mut tablet, 1)
            .await
            .is_err()
    );

    // The paired phone connects through the encrypted channel.
    let (remote, _events) = Client::connect_remote("127.0.0.1", Some(&phone), "phone")
        .await
        .unwrap();
    assert!(remote.list_workspace(false).await.is_ok());
    let status = host.lan_status().await.unwrap();
    assert!(status.devices[0].connected && status.devices[0].last_seen_ms > 0);

    // A device that knows the host's key but never paired is refused.
    let stranger = LanHost {
        host_name: String::new(),
        ..paired.clone()
    };
    let Err(err) = Client::connect_lan(&stranger, &tablet, "stranger").await else {
        panic!("an unpaired device connected");
    };
    assert!(
        matches!(err, ClientError::Lan(LanError::Refused)),
        "{err:?}"
    );

    // Plain, unencrypted protocol on the LAN port goes nowhere, and says why.
    let Err(err) = tokio::time::timeout(TIMEOUT, Client::connect_remote(&spec, None, "plain"))
        .await
        .unwrap()
    else {
        panic!("a plain connection got through");
    };
    assert!(err.to_string().contains("must be paired"), "{err}");

    // Revoking ends the live connection, and the device can't come back.
    let device_id = status.devices[0].id.clone();
    let status = host.revoke_lan_device(&device_id).await.unwrap();
    assert!(status.devices.is_empty());
    closes(&remote).await;
    let Err(err) = Client::connect_lan(&paired, &phone, "phone").await else {
        panic!("a revoked device connected");
    };
    assert!(
        matches!(err, ClientError::Lan(LanError::Refused)),
        "{err:?}"
    );
    let err = host.revoke_lan_device(&device_id).await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::NotFound));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lan_device_cannot_widen_lan_access() {
    let home = temp_home();
    let daemon = start(&home.path().join("home")).await;
    let (host, _events) = daemon.connect().await;
    let port = 47_502;
    enable(&host, port).await;
    let dir = tempfile::tempdir().unwrap();
    let mut phone = LanKeyring::open(dir.path()).unwrap();
    let code = new_code(&host).await;
    pair_lan(&format!("127.0.0.1:{port}"), &code, "phone", &mut phone, 1)
        .await
        .unwrap();
    let (remote, _events) = Client::connect_remote("127.0.0.1", Some(&phone), "phone")
        .await
        .unwrap();

    let denied = |result: Result<v1::LanStatus, ClientError>| {
        assert_eq!(
            result.unwrap_err().code(),
            Some(v1::ErrorCode::PermissionDenied)
        );
    };
    denied(remote.start_lan_pairing().await);
    denied(remote.cancel_lan_pairing().await);
    denied(remote.revoke_lan_device("x").await);
    denied(remote.set_lan_enabled(false).await);
    let err = remote
        .update_settings(v1::SettingsPatch {
            lan_port: Some(1234),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::PermissionDenied));
    let err = remote.shutdown_daemon().await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::PermissionDenied));

    // A device sees the status, but never a pairing code.
    new_code(&host).await;
    let status = remote.lan_status().await.unwrap();
    assert!(status.pairing.is_none());
    assert_eq!(status.devices.len(), 1);
    // Other settings still work from the device.
    assert!(
        remote
            .update_settings(v1::SettingsPatch {
                delete_remote_branches: Some(false),
                ..Default::default()
            })
            .await
            .is_ok()
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_lan_access_off_closes_everything() {
    let home = temp_home();
    let daemon = start(&home.path().join("home")).await;
    let (host, _events) = daemon.connect().await;
    let port = 47_503;
    enable(&host, port).await;
    let dir = tempfile::tempdir().unwrap();
    let mut phone = LanKeyring::open(dir.path()).unwrap();
    let code = new_code(&host).await;
    let paired = pair_lan(&format!("127.0.0.1:{port}"), &code, "phone", &mut phone, 1)
        .await
        .unwrap();
    let (remote, _events) = Client::connect_lan(&paired, &phone, "phone").await.unwrap();

    let status = host.set_lan_enabled(false).await.unwrap();
    assert!(!status.enabled && !status.serving);
    closes(&remote).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err(),
        "nothing listens any more"
    );

    // Switched on again, the pairing still holds (the key was kept).
    enable(&host, port).await;
    let (again, _events) = Client::connect_lan(&paired, &phone, "phone").await.unwrap();
    assert!(again.list_workspace(false).await.is_ok());
    daemon.stop().await;
}
