//! Reading the latest release from GitHub's REST API.
//!
//! `GET /repos/{owner}/{repo}/releases/latest` answers the newest release that is
//! neither a draft nor a pre-release, without credentials for a public repository. The
//! release workflow names the macOS file `Urushi-<version>-macos-<arch>.zip` and puts
//! its SHA-256 beside it in `<file>.sha256` (`docs/features/ci-and-releases.md`); both
//! names are a contract with this module.

use serde::Deserialize;
use urushi_core::Version;

use crate::http::{HttpClient, Method, Request};

/// Where the latest release of Urushi is read from.
pub const DEFAULT_FEED: &str = "https://api.github.com/repos/atgehrhardt/Urushi/releases/latest";

/// A release with a build for this Mac.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version, from the tag.
    pub version: Version,
    /// Its page on GitHub, with the notes.
    pub page_url: String,
    /// The zipped `Urushi.app`.
    pub zip_url: String,
    /// The zip's SHA-256, as `shasum` prints it.
    pub sha256_url: String,
}

/// The name of the zipped app for `version` on `arch` (`arm64`, `x86_64`).
pub fn zip_name(version: Version, arch: &str) -> String {
    format!("Urushi-{version}-macos-{arch}.zip")
}

/// This Mac's architecture as release files name it, or `None` for one never built.
pub fn mac_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Some("arm64"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
}

/// Asks `feed` for the latest release. Blocking.
pub fn latest(http: &dyn HttpClient, feed: &str, arch: &str) -> Result<Release, String> {
    let request = Request::new(
        Method::Get,
        feed.to_owned(),
        vec![
            ("Accept".into(), "application/vnd.github+json".into()),
            ("User-Agent".into(), "Urushi-updater".into()),
            ("X-GitHub-Api-Version".into(), "2022-11-28".into()),
        ],
    );
    let response = http.send(&request).map_err(|e| e.0)?;
    match response.status {
        200 => parse(&response.body, arch),
        404 => Err("there are no releases yet".into()),
        403 | 429 => Err("GitHub is limiting requests from this network; try again later".into()),
        status => Err(format!("GitHub answered {status}")),
    }
}

#[derive(Deserialize)]
struct WireRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<WireAsset>,
}

#[derive(Deserialize)]
struct WireAsset {
    name: String,
    browser_download_url: String,
}

/// The release in GitHub's answer, with the files for `arch`.
pub fn parse(body: &[u8], arch: &str) -> Result<Release, String> {
    let release: WireRelease =
        serde_json::from_slice(body).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    if release.draft || release.prerelease {
        return Err("the latest release is a draft or a pre-release".into());
    }
    let version: Version = release.tag_name.parse().map_err(|_| {
        format!(
            "the latest release's tag {:?} isn't a version",
            release.tag_name
        )
    })?;
    let zip = zip_name(version, arch);
    let sha = format!("{zip}.sha256");
    let url_of = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    let (Some(zip_url), Some(sha256_url)) = (url_of(&zip), url_of(&sha)) else {
        return Err(format!(
            "release {version} has no macOS build for this Mac ({zip} and its .sha256)"
        ));
    };
    Ok(Release {
        version,
        page_url: release.html_url,
        zip_url,
        sha256_url,
    })
}

/// The SHA-256 in a `.sha256` file: its first word, 64 hex digits, lowercased.
pub fn parse_sha256(text: &str) -> Result<String, String> {
    let digest = text.split_whitespace().next().unwrap_or_default();
    if digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(digest.to_ascii_lowercase())
    } else {
        Err("the release's checksum file is malformed".into())
    }
}
