//! Reading releases, deciding what to offer, and installing into a bundle in a temp
//! folder. Installing uses macOS's own `ditto`, `codesign` and `shasum`, so those tests
//! return early where they're missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use urushi_core::Version;

use super::bundle::{self, BUNDLE_ID, test_plist};
use super::github::{self, Release};
use super::{UNSUPPORTED, UpdateConfig, Updater};
use crate::http::{HttpClient, HttpError, Request, Response};

const FEED: &str = "https://api.example/releases/latest";

/// Answers each URL with a fixed status and body, and counts requests.
#[derive(Default)]
struct Web {
    pages: Mutex<HashMap<String, (u16, Vec<u8>)>>,
    asked: Mutex<Vec<String>>,
}

impl Web {
    fn serve(&self, url: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.pages
            .lock()
            .unwrap()
            .insert(url.to_owned(), (status, body.into()));
    }
}

impl HttpClient for Web {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        self.asked.lock().unwrap().push(request.url.clone());
        let (status, body) = self
            .pages
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
    github::mac_arch().unwrap_or("arm64")
}

fn release_json(tag: &str, arch: &str) -> String {
    let zip = format!("Urushi-{}-macos-{arch}.zip", tag.trim_start_matches('v'));
    serde_json::json!({
        "tag_name": tag,
        "html_url": format!("https://github.example/releases/tag/{tag}"),
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": "Urushi-x-android-arm64.apk", "browser_download_url": "https://dl/apk"},
            {"name": zip, "browser_download_url": "https://dl/zip"},
            {"name": format!("{zip}.sha256"), "browser_download_url": "https://dl/sha"},
        ],
    })
    .to_string()
}

/// A bundle at `dir/Urushi.app` saying `id` and `version`, with every executable.
fn make_bundle(dir: &Path, id: &str, version: &str) -> PathBuf {
    let app = dir.join("Urushi.app");
    for exe in bundle::EXECUTABLES {
        let path = app.join(exe);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A real Mach-O where there is one, so `codesign` can sign it.
        if std::fs::copy("/usr/bin/true", &path).is_err() {
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
        }
    }
    std::fs::write(app.join("Contents/Info.plist"), test_plist(id, version)).unwrap();
    app
}

fn updater(web: &Arc<Web>, bundle: Option<PathBuf>) -> Updater {
    Updater::new(UpdateConfig {
        http: web.clone(),
        feed_url: FEED.into(),
        bundle,
    })
}

fn v(s: &str) -> Version {
    s.parse().unwrap()
}

// ---- reading releases ---------------------------------------------------------------

#[test]
fn a_release_gives_its_version_page_and_this_macs_files() {
    let release = github::parse(release_json("v0.1.7", "arm64").as_bytes(), "arm64").unwrap();
    assert_eq!(
        release,
        Release {
            version: v("0.1.7"),
            page_url: "https://github.example/releases/tag/v0.1.7".into(),
            zip_url: "https://dl/zip".into(),
            sha256_url: "https://dl/sha".into(),
        }
    );
}

#[test]
fn releases_without_a_build_for_this_mac_or_a_version_tag_are_refused() {
    let arm = release_json("v0.1.7", "arm64");
    let error = github::parse(arm.as_bytes(), "x86_64").unwrap_err();
    assert!(error.contains("no macOS build for this Mac"), "{error}");
    let error = github::parse(release_json("nightly", "arm64").as_bytes(), "arm64").unwrap_err();
    assert!(error.contains("isn't a version"), "{error}");
    let mut draft: serde_json::Value = serde_json::from_str(&arm).unwrap();
    draft["prerelease"] = true.into();
    assert!(github::parse(draft.to_string().as_bytes(), "arm64").is_err());
    assert!(github::parse(b"<html>", "arm64").is_err());
}

#[test]
fn github_failures_are_explained() {
    let web = Arc::new(Web::default());
    web.serve(FEED, 404, "{}");
    let error = github::latest(web.as_ref(), FEED, "arm64").unwrap_err();
    assert_eq!(error, "there are no releases yet");
    web.serve(FEED, 403, "{}");
    let error = github::latest(web.as_ref(), FEED, "arm64").unwrap_err();
    assert!(error.contains("limiting requests"), "{error}");
    let error = github::latest(web.as_ref(), "https://elsewhere", "arm64").unwrap_err();
    assert_eq!(error, "offline");
}

#[test]
fn checksum_files_are_read_as_shasum_writes_them() {
    let digest = "AB".repeat(32);
    assert_eq!(
        github::parse_sha256(&format!("{digest}  Urushi-0.1.7-macos-arm64.zip\n")),
        Ok(digest.to_ascii_lowercase())
    );
    assert!(github::parse_sha256("").is_err());
    assert!(github::parse_sha256("abc  file").is_err());
    assert!(github::parse_sha256(&format!("{}  f", "zz".repeat(32))).is_err());
}

// ---- the bundle -----------------------------------------------------------------------

#[test]
fn info_plists_are_read_as_the_bundle_script_writes_them() {
    let plist = test_plist("app.urushi.Urushi", "0.1.6");
    assert_eq!(
        bundle::string_value(&plist, "CFBundleIdentifier").as_deref(),
        Some("app.urushi.Urushi")
    );
    assert_eq!(
        bundle::string_value(
            "<key>K</key>\n  <string> a &amp; b &lt;c&gt; </string>",
            "K"
        )
        .as_deref(),
        Some("a & b <c>")
    );
    assert_eq!(bundle::string_value("<key>K</key><true/>", "K"), None);
    assert_eq!(bundle::string_value(&plist, "Missing"), None);
}

#[test]
fn only_a_daemon_inside_urushis_own_bundle_has_one() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_bundle(dir.path(), BUNDLE_ID, "0.1.6")
        .canonicalize()
        .unwrap();
    assert_eq!(
        bundle::bundle_of(&app.join("Contents/MacOS/urushid")),
        Some(app.clone())
    );
    assert_eq!(
        bundle::bundle_of(Path::new("/repo/target/debug/urushid")),
        None
    );
    assert_eq!(
        bundle::bundle_of(&app.join("Contents/Resources/bin/urushi")),
        None
    );
    // The CLI starts the daemon as `Resources/bin/../../MacOS/urushid`.
    let via_cli = app.join("Contents/Resources/bin/../../MacOS/urushid");
    assert_eq!(bundle::bundle_of(&via_cli), Some(app.clone()));
    let other = dir.path().join("other");
    let foreign = make_bundle(&other, "com.example.Other", "1.0.0");
    assert_eq!(
        bundle::bundle_of(&foreign.join("Contents/MacOS/urushid")),
        None
    );
}

// ---- deciding -------------------------------------------------------------------------

#[test]
fn a_newer_release_is_offered_and_an_equal_or_older_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_bundle(dir.path(), BUNDLE_ID, "0.1.6");
    let web = Arc::new(Web::default());
    let updater = updater(&web, Some(app));
    assert_eq!(updater.installed_version(), v("0.1.6"));

    web.serve(FEED, 200, release_json("v0.1.7", arch()));
    let (changed, check) = updater.check().unwrap();
    assert!(changed && check.available());
    let offered = updater.to_proto().unwrap();
    assert_eq!(
        (offered.version.as_str(), offered.current_version.as_str()),
        ("0.1.7", "0.1.6")
    );
    assert!(!offered.installing);
    let (changed, _) = updater.check().unwrap();
    assert!(!changed, "finding the same release again changes nothing");

    for tag in ["v0.1.6", "v0.1.5"] {
        web.serve(FEED, 200, release_json(tag, arch()));
        let (_, check) = updater.check().unwrap();
        assert!(!check.available(), "{tag}");
        assert_eq!(updater.to_proto(), None, "{tag}");
    }
}

#[test]
fn a_daemon_outside_a_bundle_never_asks_github() {
    let web = Arc::new(Web::default());
    let updater = updater(&web, None);
    assert_eq!(updater.check().unwrap_err(), UNSUPPORTED);
    assert!(web.asked.lock().unwrap().is_empty());
    assert!(updater.begin_install(v("0.1.7")).is_err());
}

#[test]
fn only_the_release_found_is_installed_and_only_once_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_bundle(dir.path(), BUNDLE_ID, "0.1.6");
    let web = Arc::new(Web::default());
    let updater = updater(&web, Some(app));
    let error = updater.begin_install(v("0.1.7")).unwrap_err();
    assert!(error.contains("check again"), "{error}");

    web.serve(FEED, 200, release_json("v0.1.8", arch()));
    updater.check().unwrap();
    let error = updater.begin_install(v("0.1.7")).unwrap_err();
    assert!(error.contains("0.1.8 is"), "{error}");
    updater.begin_install(v("0.1.8")).unwrap();
    assert!(updater.to_proto().unwrap().installing);
    assert!(updater.begin_install(v("0.1.8")).is_err(), "one at a time");

    updater.finish_install(false);
    assert!(
        !updater.to_proto().unwrap().installing,
        "a failure can be retried"
    );
    updater.begin_install(v("0.1.8")).unwrap();
    updater.finish_install(true);
    assert_eq!(
        updater.to_proto(),
        None,
        "installed: nothing newer any more"
    );
}

// ---- installing (macOS tools) ---------------------------------------------------------

fn have_mac_tools() -> bool {
    ["/usr/bin/ditto", "/usr/bin/codesign", "/usr/bin/shasum"]
        .iter()
        .all(|t| Path::new(t).exists())
}

/// A web serving a signed, zipped bundle of `id`/`version` (with its checksum, or
/// `sha` instead; changed after signing if `tamper`), and the release found for it.
fn serve_release(
    dir: &Path,
    id: &str,
    version: &str,
    sha: Option<&str>,
    tamper: bool,
) -> (Arc<Web>, Release) {
    let build = dir.join("build");
    let app = make_bundle(&build, id, version);
    let signed = std::process::Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(signed.success());
    if tamper {
        let daemon = app.join("Contents/MacOS/urushid");
        let mut bytes = std::fs::read(&daemon).unwrap();
        bytes.extend_from_slice(b"injected");
        std::fs::write(&daemon, bytes).unwrap();
    }
    let zip = dir.join("release.zip");
    let zipped = std::process::Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&zip)
        .status()
        .unwrap();
    assert!(zipped.success());
    let out = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(&zip)
        .output()
        .unwrap();
    let digest = String::from_utf8(out.stdout).unwrap();
    let web = Arc::new(Web::default());
    web.serve("https://dl/zip", 200, std::fs::read(&zip).unwrap());
    web.serve("https://dl/sha", 200, sha.map_or(digest, str::to_owned));
    let release = Release {
        version: v("0.1.7"),
        page_url: "https://github.example/releases/tag/v0.1.7".into(),
        zip_url: "https://dl/zip".into(),
        sha256_url: "https://dl/sha".into(),
    };
    (web, release)
}

/// An installed 0.1.6 with a marker file, in `dir/Applications`.
fn installed(dir: &Path) -> PathBuf {
    let app = make_bundle(&dir.join("Applications"), BUNDLE_ID, "0.1.6");
    std::fs::write(app.join("Contents/old-marker"), "old").unwrap();
    app
}

fn leftovers(app: &Path) -> Vec<String> {
    std::fs::read_dir(app.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "Urushi.app")
        .collect()
}

#[test]
fn installing_puts_the_checked_release_in_place_of_the_app() {
    if !have_mac_tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let app = installed(dir.path());
    let (web, release) = serve_release(dir.path(), BUNDLE_ID, "0.1.7", None, false);
    let work = dir.path().join("home/updates");
    super::install::install(web.as_ref(), &release, &app, &work).unwrap();

    assert_eq!(bundle::info(&app).unwrap().version, Some(v("0.1.7")));
    assert!(
        !app.join("Contents/old-marker").exists(),
        "the old app is gone"
    );
    assert!(leftovers(&app).is_empty(), "{:?}", leftovers(&app));
    assert!(!work.exists(), "the download is cleaned up");
}

#[test]
fn a_damaged_or_wrong_release_leaves_the_app_alone() {
    if !have_mac_tools() {
        return;
    }
    let cases = [
        (BUNDLE_ID, "0.1.7", Some("0".repeat(64)), false, "damaged"),
        (BUNDLE_ID, "0.1.9", None, false, "isn't version 0.1.7"),
        ("com.example.Other", "0.1.7", None, false, "isn't Urushi"),
        (BUNDLE_ID, "0.1.7", None, true, "signature is broken"),
    ];
    for (id, version, sha, tamper, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let app = installed(dir.path());
        let (web, release) = serve_release(dir.path(), id, version, sha.as_deref(), tamper);
        let work = dir.path().join("home/updates");
        let error = super::install::install(web.as_ref(), &release, &app, &work).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
        assert!(app.join("Contents/old-marker").exists(), "{expected}");
        assert_eq!(bundle::info(&app).unwrap().version, Some(v("0.1.6")));
        assert!(
            leftovers(&app).is_empty(),
            "{expected}: {:?}",
            leftovers(&app)
        );
        assert!(!work.exists());
    }
}

#[test]
fn a_failed_download_is_explained_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let app = installed(dir.path());
    let web = Arc::new(Web::default());
    web.serve("https://dl/sha", 200, format!("{}  f", "a".repeat(64)));
    web.serve("https://dl/zip", 404, "Not Found");
    let release = Release {
        version: v("0.1.7"),
        page_url: String::new(),
        zip_url: "https://dl/zip".into(),
        sha256_url: "https://dl/sha".into(),
    };
    let error =
        super::install::install(web.as_ref(), &release, &app, &dir.path().join("w")).unwrap_err();
    assert!(error.contains("couldn't download the update"), "{error}");
    assert!(app.join("Contents/old-marker").exists());
}
