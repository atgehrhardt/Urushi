# Feature: CI and releases

- **Status:** Shipped (CI on every push and pull request; macOS, Android and unsigned iOS releases from tags)
- **Owners (crates):** none; `.github/workflows/`, `scripts/bundle-macos.sh`, `scripts/build-android.sh`, `scripts/bundle-ios.sh`, `Makefile`
- **Related ADRs:** [0032](../adr/0032-tagged-macos-releases.md), [0040](../adr/0040-release-android-apks-signed-with-a-repository-secret-key.md), [0045](../adr/0045-release-the-unsigned-ios-ipa-for-sideloading.md), [0007](../adr/0007-platform-isolation.md)

## What it does
Every push to `main` and every pull request runs the same checks as `make ci`, on macOS,
Linux and Windows, and builds the Android APK and the iOS app. Pushing a tag `vX.Y.Z` builds
`Urushi.app` for Apple Silicon, the Android APK and the unsigned iOS `.ipa`, and publishes
them as one GitHub Release. Release notes explain how to sideload the `.ipa`.

## How it works
**CI** (`.github/workflows/ci.yml`):

| Job | Runs on | What |
|---|---|---|
| `test (<os>)` | macOS, Ubuntu, Windows | `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --all-targets --no-fail-fast` (every failing test binary is reported, not just the first), doc tests |
| `ios` | macOS | `scripts/bundle-ios.sh` (simulator) and `scripts/bundle-ios.sh --device`. Uploads the unsigned `Urushi.ipa` (`urushi-ios-unsigned`) and the zipped simulator app (`urushi-ios-simulator`), kept 14 days. |
| `android` | Ubuntu | `scripts/build-android.sh` with the runner's SDK, NDK and JDK. Uploads the debug-signed APK as the `urushi-android-debug` artifact (kept 14 days). |
| `docs` | Ubuntu | `scripts/check-docs.sh` |
| `licenses` | Ubuntu | `cargo deny check` |
| `proto` | Ubuntu | `buf lint`; on pull requests, also `buf breaking` against `main` |

The test jobs time out after 45 minutes, so a hung test fails the run. The toolchain comes from `rust-toolchain.toml`. The daemon's end-to-end tests
(`crates/urushi-daemon/tests/*.rs`) are Unix-only, because they drive `/bin/sh` and git
fixtures. On Windows the unit tests and the `cmd.exe` PTY test run.

**Releases** (`.github/workflows/release.yml`, ADRs 0032, 0040 and 0045):
1. Bump `[workspace.package].version` in `Cargo.toml` and commit it. Wait for CI to be green.
   The APK's `versionName` and `versionCode` follow automatically.
2. Run `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The workflow's jobs:

   | Job | Runs on | What |
   |---|---|---|
   | `version` | Ubuntu | Refuses a tag that doesn't match the version |
   | `macos` | macOS | `scripts/bundle-macos.sh` (release profile, ad-hoc `codesign`), checks the bundle, zips it |
   | `android` | Ubuntu | `scripts/build-android.sh` with the release key, then checks the APK's `versionName`, its arm64 library and its signature |
   | `ios` | macOS | `scripts/bundle-ios.sh --device` (release, arm64, unsigned), then checks the executable's architecture, `CFBundleShortVersionString` and bundle identifier |
   | `publish` | Ubuntu | Tags only, after all three builds pass: uploads `Urushi-X.Y.Z-macos-arm64.zip`, `Urushi-X.Y.Z-android-arm64.apk`, `Urushi-X.Y.Z-ios-arm64-unsigned.ipa` and their `.sha256` files to a new release, with install and sideloading notes and generated change notes |

**Dry run:** Actions → Release → *Run workflow* builds the same files as workflow artifacts
and publishes nothing. Pull requests that change `release.yml`, `scripts/bundle-macos.sh`,
`scripts/build-android.sh`, `scripts/bundle-ios.sh` or `apps/android/AndroidManifest.xml` get the same dry run
automatically.

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
Tags fail without these secrets. Manual runs use them when they're set. Pull requests never
get them, so their APKs are debug-signed. Locally, `scripts/build-android.sh` signs with
`ANDROID_KEYSTORE` (and the same password and alias variables) when set. Otherwise it uses
Android's standard debug key, `~/.android/debug.keystore`.

## Invariants
- A release's version, its tag, the apps' `CFBundleShortVersionString` and the APK's
  `versionName` are equal. The `version` job refuses a mismatched tag, and
  `build-android.sh` and `bundle-ios.sh` stamp their builds from `Cargo.toml`. The
  `android` and `ios` jobs check it.
- A release contains every platform or none: `publish` needs all three build jobs.
- Release APKs are signed with the release key, never a debug key (the `android` job fails
  on a tag without it).
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
  racing it.

## Tests
- The workflows themselves are exercised by running them: CI on every pull request, and
  the release workflow's dry run on pull requests that change how releases are built.
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
- The release job doesn't re-run the tests. Tag only commits where CI passed.
