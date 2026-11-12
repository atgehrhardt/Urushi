# Feature: CI and releases

- **Status:** Shipped (CI on every push and pull request; app builds on every merge to `main`; macOS, Android and unsigned iOS releases from a button)
- **Owners (crates):** none; `.github/workflows/`, `scripts/bundle-macos.sh`, `scripts/build-android.sh`, `scripts/bundle-ios.sh`, `Makefile`
- **Related ADRs:** [0053](../adr/0053-build-every-merge-release-from-a-button.md), [0032](../adr/0032-tagged-macos-releases.md), [0040](../adr/0040-release-android-apks-signed-with-a-repository-secret-key.md), [0045](../adr/0045-release-the-unsigned-ios-ipa-for-sideloading.md), [0007](../adr/0007-platform-isolation.md)

## What it does
Every push to `main` and every pull request runs the same checks as `make ci`, on macOS,
Linux and Windows. Pull requests also build the Android APK and the iOS app.

Every merge to `main` builds every app a release would ship: `Urushi.app` for Apple
Silicon, the Android APK and the unsigned iOS `.ipa`, plus the iOS Simulator app. They are
kept as workflow artifacts for 30 days and never published.

A release is started by hand. Actions → Release → *Run workflow* on `main` builds the
version in `Cargo.toml` and publishes it as one GitHub Release, tagged `vX.Y.Z`. The
release notes explain how to sideload the `.ipa`.

## How it works
**CI** (`.github/workflows/ci.yml`):

| Job | Runs on | What |
|---|---|---|
| `test (<os>)` | macOS, Ubuntu, Windows | `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --all-targets --no-fail-fast` (every failing test binary is reported, not just the first), doc tests |
| `ios` | macOS | Pull requests only. `scripts/bundle-ios.sh` (simulator) and `scripts/bundle-ios.sh --device`. Uploads the unsigned `Urushi.ipa` (`urushi-ios-unsigned`) and the zipped simulator app (`urushi-ios-simulator`), kept 14 days. |
| `android` | Ubuntu | Pull requests only. `scripts/build-android.sh` with the runner's SDK, NDK and JDK. Uploads the debug-signed APK as the `urushi-android-debug` artifact (kept 14 days). |
| `docs` | Ubuntu | `scripts/check-docs.sh` |
| `licenses` | Ubuntu | `cargo deny check` |
| `proto` | Ubuntu | `buf lint`; on pull requests, also `buf breaking` against `main` |

The test jobs time out after 45 minutes, so a hung test fails the run. The toolchain comes from `rust-toolchain.toml`. The daemon's end-to-end tests
(`crates/urushi-daemon/tests/*.rs`) are Unix-only, because they drive `/bin/sh` and git
fixtures. On Windows the unit tests and the `cmd.exe` PTY test run.

**Building the apps** (`.github/workflows/build-apps.yml`, reusable). The Build and Release
workflows both call it, so a release is built exactly like every merge:

| Job | Runs on | What |
|---|---|---|
| `version` | Ubuntu | Reads `[workspace.package].version` and names the files: `Urushi-X.Y.Z-…` for a release, `Urushi-X.Y.Z-<commit>-…` otherwise |
| `macos` | macOS | `scripts/bundle-macos.sh` (release profile, ad-hoc `codesign`). Checks the binaries, arm64, the bundle version and the signature, then zips it. |
| `android` | Ubuntu | `scripts/build-android.sh`, then checks the APK's `versionName`, its arm64 library and its signature. A release requires the release key; anything else is signed with the debug key. |
| `ios` | macOS | `scripts/bundle-ios.sh --device` (release, arm64, unsigned), then checks the executable's architecture, `CFBundleShortVersionString` and bundle identifier. With `simulator`, it also zips the Simulator app. |

Each file is uploaded as its own artifact, with its `.sha256` (`shasum` format).

**Builds** (`.github/workflows/build.yml`, "Build", ADR 0053) run on every push to `main`,
which means every merged pull request. They upload the macOS zip, the debug-signed APK, the
unsigned `.ipa` and the Simulator zip, kept 30 days. Download them from the run's
*Artifacts*. Pull requests that change any of these get the same build:
- `build.yml`, `build-apps.yml` or `release.yml`;
- `scripts/bundle-macos.sh`, `scripts/build-android.sh` or `scripts/bundle-ios.sh`;
- `apps/android/AndroidManifest.xml`.

A newer push to a pull request cancels its running build. Builds of `main` are never
cancelled.

**Releases** (`.github/workflows/release.yml`, "Release", ADRs 0053, 0032, 0040 and 0045):
1. Bump `[workspace.package].version` in `Cargo.toml` in a pull request and merge it. The
   apps' versions, and the APK's `versionName` and `versionCode`, follow automatically.
2. Wait for CI on `main` to go green.
3. Actions → Release → *Run workflow*, with `main` selected.
4. The workflow's jobs:

   | Job | Runs on | What |
   |---|---|---|
   | `preflight` | Ubuntu | Refuses if the run isn't on `main`, if `vX.Y.Z` is already tagged, or if CI for the commit hasn't finished successfully |
   | `build` | (reusable) | `build-apps.yml` with `release: true` and the release key |
   | `publish` | Ubuntu | After every build passes: creates tag `vX.Y.Z` on the built commit and a release holding `Urushi-X.Y.Z-macos-arm64.zip`, `Urushi-X.Y.Z-android-arm64.apk`, `Urushi-X.Y.Z-ios-arm64-unsigned.ipa` and their `.sha256` files, with install and sideloading notes and generated change notes |

   Releases run one at a time. A second click waits, then finds the tag and stops.

Pushing a tag doesn't trigger anything.

**The Android release key** (one-time setup). Android only installs an update signed with
the same key, so keep the keystore and its password safe outside the repo. Losing them
means users have to uninstall to upgrade.
```sh
keytool -genkeypair -keystore urushi-release.jks -alias urushi -keyalg RSA -keysize 4096 \
  -validity 10000 -dname "CN=Urushi"
base64 -i urushi-release.jks | gh secret set ANDROID_KEYSTORE_BASE64
gh secret set ANDROID_KEYSTORE_PASSWORD     # prompts for the password
gh secret set ANDROID_KEY_ALIAS --body urushi
# ANDROID_KEY_PASSWORD is only needed if the key's password differs from the keystore's.
```
Releases fail without these secrets. Only the Release workflow gets them, so every
other APK (CI, Build, pull requests) is debug-signed. Locally, `scripts/build-android.sh` signs with
`ANDROID_KEYSTORE` (and the same password and alias variables) when set. Otherwise it uses
Android's standard debug key, `~/.android/debug.keystore`.

## Invariants
- The macOS updater depends on what releases publish (ADR 0049,
  [updates](updates.md)): a tag `vX.Y.Z`, `Urushi-X.Y.Z-macos-arm64.zip` holding one
  `Urushi.app` whose `CFBundleShortVersionString` is `X.Y.Z`, and its `.sha256` in
  `shasum`'s format. `releases/latest` must not be a draft or a pre-release. Renaming
  these breaks updating for every installed app.
- A release's version, its tag, the apps' `CFBundleShortVersionString` and the APK's
  `versionName` are equal. The tag is created from `Cargo.toml`'s version, and
  `bundle-macos.sh`, `build-android.sh` and `bundle-ios.sh` stamp their builds from it.
  The `macos`, `android` and `ios` jobs check it.
- A release contains every platform or none: `publish` needs all three build jobs.
- Release APKs are signed with the release key, never a debug key (the `android` job fails
  a release without it). Nothing but a release is signed with it.
- A version is released once (`preflight` refuses an existing tag), only from `main`, and
  only from a commit whose CI passed.
- Releases are built by the same scripts as `make app`, `make install`, `make android` and `make ios`,
  so a released app is laid out exactly like a local one.
- `#[cfg(target_os/unix/windows)]` stays inside `platform/` modules (ADR 0007). CI
  compiles all three desktop OSes, which catches Unix-only APIs used outside them (e.g.
  `MasterPty::tty_name`, now `urushi_mux::platform::tty_path`).
- Paths the daemon stores or hands to git are canonicalized with
  `urushi_daemon::platform::canonicalize`, never `std::fs::canonicalize` directly. On
  Windows that strips the verbatim `\\?\` prefix, which git can't use.
- Helper programs' failures read `… failed (exit status: N)` on every OS (`exec::run`), so
  `http::friendly` recognizes curl's exit codes on Windows too.
- Daemon integration tests don't depend on timing: the test helpers report a daemon that
  failed to start with its actual error (`tests/common/mod.rs`). Transient states are
  observed by holding the fake service in place (`FakeJira::hold_transitions`), not by
  racing it. Output in a pane is matched anywhere on a line, because a command typed before
  the shell's first prompt prints its output behind that prompt (`args_printed` in
  `tests/task_agents.rs`).

## Tests
- The workflows themselves are exercised by running them: CI on every pull request, and
  Build on every merge and on pull requests that change how the apps are built. Only a
  real release runs `preflight` and `publish`. Running Release for an already released
  version safely exercises `preflight`, which stops before building.
- Before pushing, `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc`
  (and `x86_64-unknown-linux-gnu`) type-checks the other desktop OSes from a Mac.

## Limitations and follow-ups
- The app isn't notarized. Users allow the first launch by hand. Notarization needs an
  Apple Developer ID certificate and notary credentials as repository secrets.
- Only Apple Silicon is released. Intel Macs would need an `x86_64-apple-darwin` build
  merged with `lipo`.
- There are no Windows or Linux releases, and no store builds. The iOS `.ipa` is unsigned:
  each user re-signs it with their own Apple ID, every 7 days with a free one
  ([platforms](../platforms.md), ADR 0045). A signed build or TestFlight needs a paid Apple
  Developer account.
- Only arm64 Android devices are supported (no `armeabi-v7a` or `x86_64` libraries in the
  APK).
- The release doesn't re-run the tests. It relies on CI's result for the commit, which
  `preflight` checks.
- There's no hand-started dry run. A release is the first time the release key signs a
  build. Merge builds are debug-signed.
