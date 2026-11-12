//! App updates end to end (protocol 1.28, ADR 0049): a daemon running from a bundle in
//! a temp folder finds a newer release on a fake GitHub, tells subscribers through the
//! workspace, and installs it on request; one outside a bundle refuses.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use common::{Daemon, temp_home};
use urushi_client::v1;
use urushi_daemon::DaemonOptions;
use urushi_daemon::http::{HttpClient, HttpError, Request, Response};
use urushi_daemon::tailscale::StaticTailscale;
use urushi_daemon::update::UpdateConfig;
use urushi_daemon::update::bundle::EXECUTABLES;

const FEED: &str = "https://api.example/releases/latest";

#[derive(Default)]
struct Web(Mutex<HashMap<String, (u16, Vec<u8>)>>);

impl Web {
    fn serve(&self, url: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.0
            .lock()
            .unwrap()
            .insert(url.to_owned(), (status, body.into()));
    }
}

impl HttpClient for Web {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        let (status, body) = self
            .0
            .lock()
            .unwrap()
            .get(&request.url)
            .cloned()
            .ok_or_else(|| HttpError("offline".into()))?;
        Ok(Response {
            status,
            retry_after: None,
            body,
        })
    }
}

fn arch() -> &'static str {
    urushi_daemon::update::github::mac_arch().unwrap_or("arm64")
}

fn serve_release(web: &Web, tag: &str) {
    let zip = format!(
        "Urushi-{}-macos-{}.zip",
        tag.trim_start_matches('v'),
        arch()
    );
    let body = serde_json::json!({
        "tag_name": tag,
        "html_url": format!("https://github.example/releases/tag/{tag}"),
        "assets": [
            {"name": zip, "browser_download_url": "https://dl/zip"},
            {"name": format!("{zip}.sha256"), "browser_download_url": "https://dl/sha"},
        ],
    });
    web.serve(FEED, 200, body.to_string());
}

fn plist(version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n  \
         <key>CFBundleIdentifier</key><string>app.urushi.Urushi</string>\n  \
         <key>CFBundleShortVersionString</key><string>{version}</string>\n</dict>\n</plist>\n"
    )
}

fn make_bundle(dir: &Path, version: &str) -> PathBuf {
    let app = dir.join("Urushi.app");
    for exe in EXECUTABLES {
        let path = app.join(exe);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if std::fs::copy("/usr/bin/true", &path).is_err() {
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
        }
    }
    std::fs::write(app.join("Contents/Info.plist"), plist(version)).unwrap();
    app
}

async fn start(home: &Path, web: Arc<Web>, bundle: Option<PathBuf>) -> Daemon {
    let mut options = DaemonOptions::new(home.to_path_buf());
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.updates = UpdateConfig {
        http: web,
        feed_url: FEED.into(),
        bundle,
    };
    Daemon::start_options(options).await
}

#[tokio::test]
async fn a_daemon_outside_an_app_bundle_has_no_updates() {
    let home = temp_home();
    let web = Arc::new(Web::default());
    serve_release(&web, "v99.0.0");
    let daemon = start(home.path(), web, None).await;
    let (client, _events) = daemon.connect().await;

    let error = client.check_for_updates().await.unwrap_err();
    assert_eq!(error.code(), Some(v1::ErrorCode::FailedPrecondition));
    let error = client.install_update("99.0.0").await.unwrap_err();
    assert_eq!(error.code(), Some(v1::ErrorCode::FailedPrecondition));
    assert_eq!(client.list_workspace(false).await.unwrap().update, None);
    daemon.stop().await;
}

#[tokio::test]
async fn a_newer_release_reaches_clients_and_later_is_remembered() {
    let home = temp_home();
    let apps = tempfile::tempdir().unwrap();
    let app = make_bundle(apps.path(), "0.1.6");
    let web = Arc::new(Web::default());
    serve_release(&web, "v0.1.7");
    let daemon = start(home.path(), web.clone(), Some(app)).await;
    let (client, _events) = daemon.connect().await;

    let check = client.check_for_updates().await.unwrap();
    assert_eq!(
        (
            check.current_version.as_str(),
            check.latest_version.as_str()
        ),
        ("0.1.6", "0.1.7")
    );
    assert!(check.available);
    assert_eq!(
        check.release_url,
        "https://github.example/releases/tag/v0.1.7"
    );
    let update = client.list_workspace(false).await.unwrap().update.unwrap();
    assert_eq!(update.version, "0.1.7");
    assert!(!update.installing);

    // "Later": kept in the settings every client shares; the release is still known.
    let settings = client
        .update_settings(v1::SettingsPatch {
            skipped_update_version: Some("0.1.7".into()),
            check_updates_automatically: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    let updates = settings.updates.unwrap();
    assert_eq!(updates.skipped_version, "0.1.7");
    assert_eq!(updates.check_automatically, Some(false));
    assert!(client.list_workspace(false).await.unwrap().update.is_some());

    // Only the release found can be installed.
    let error = client.install_update("0.1.8").await.unwrap_err();
    assert_eq!(error.code(), Some(v1::ErrorCode::Conflict));

    // A release that isn't newer clears what clients see.
    serve_release(&web, "v0.1.6");
    assert!(!client.check_for_updates().await.unwrap().available);
    assert_eq!(client.list_workspace(false).await.unwrap().update, None);
    daemon.stop().await;
}

fn sh(program: &str, args: &[&str]) -> Vec<u8> {
    let out = Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{program} {args:?}");
    out.stdout
}

#[tokio::test]
async fn installing_replaces_the_bundle_and_the_offer_goes_away() {
    if !["/usr/bin/ditto", "/usr/bin/codesign", "/usr/bin/shasum"]
        .iter()
        .all(|t| Path::new(t).exists())
    {
        return;
    }
    let home = temp_home();
    let apps = tempfile::tempdir().unwrap();
    let app = make_bundle(&apps.path().join("Applications"), "0.1.6");
    let build = make_bundle(&apps.path().join("build"), "0.1.7");
    let path = |p: &Path| p.to_string_lossy().into_owned();
    sh(
        "codesign",
        &["--force", "--deep", "--sign", "-", &path(&build)],
    );
    let zip = apps.path().join("release.zip");
    sh(
        "ditto",
        &["-c", "-k", "--keepParent", &path(&build), &path(&zip)],
    );
    let sha = sh("shasum", &["-a", "256", &path(&zip)]);

    let web = Arc::new(Web::default());
    serve_release(&web, "v0.1.7");
    web.serve("https://dl/sha", 200, "0".repeat(64));
    web.serve("https://dl/zip", 200, std::fs::read(&zip).unwrap());
    let daemon = start(home.path(), web.clone(), Some(app.clone())).await;
    let (client, _events) = daemon.connect().await;
    assert!(client.check_for_updates().await.unwrap().available);

    // A damaged download is refused, and can be tried again.
    let error = client.install_update("0.1.7").await.unwrap_err();
    assert!(error.to_string().contains("damaged"), "{error}");
    let update = client.list_workspace(false).await.unwrap().update.unwrap();
    assert!(!update.installing);
    assert!(
        std::fs::read_to_string(app.join("Contents/Info.plist"))
            .unwrap()
            .contains("0.1.6")
    );

    web.serve("https://dl/sha", 200, sha);
    let installed = client.install_update("0.1.7").await.unwrap();
    assert_eq!(installed.version, "0.1.7");
    assert_eq!(PathBuf::from(&installed.bundle_path), app);
    assert!(
        std::fs::read_to_string(app.join("Contents/Info.plist"))
            .unwrap()
            .contains("0.1.7")
    );
    assert_eq!(client.list_workspace(false).await.unwrap().update, None);
    assert!(
        !home.path().join("updates").exists(),
        "the download is cleaned up"
    );
    // The daemon reads the installed version from the bundle, so it isn't offered again.
    assert!(!client.check_for_updates().await.unwrap().available);
    daemon.stop().await;
}
