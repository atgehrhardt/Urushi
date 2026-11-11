# 0040. Release the Android APK from the same tag, signed with a release key kept in repository secrets; no iOS release yet

- **Status:** Accepted; the "iOS isn't released" part is superseded by [0045](0045-release-the-unsigned-ios-ipa-for-sideloading.md) (releases include the unsigned `.ipa`). The Android decisions stand.
- **Date:** 2026-11-10

## Context
ADR [0032](0032-tagged-macos-releases.md) releases only macOS. The mobile clients already
build without an IDE (`scripts/build-android.sh`, `scripts/bundle-ios.sh`), and the user
asked for the release workflow to ship them too, and for CI to build Android.

Android only installs an update when it is signed with the same key as the installed app.
The build script used to sign with a debug key generated per checkout, so every worktree,
and every CI run, produced an APK that couldn't install over the last one. A real iPhone
only runs apps signed with an Apple provisioning profile, which needs a paid Apple
Developer account. The project doesn't have one.

## Decision
- **Android ships with every `vX.Y.Z` release.** `release.yml` builds the APK on
  `ubuntu-latest` alongside the macOS job and publishes `Urushi-<version>-android-arm64.apk`
  plus its `.sha256` in the same release. A separate `publish` job creates the release
  only after both builds pass, so a release never contains just one platform.
- **The APK's version comes from `Cargo.toml`.** `build-android.sh` passes `versionName`
  and `versionCode` (`MAJOR*1000000 + MINOR*1000 + PATCH`) to `aapt2 link
  --replace-version`. The manifest no longer carries them, so the APK can't disagree
  with the tag.
- **Release builds are signed with a release key from repository secrets**
  (`ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, optional
  `ANDROID_KEY_PASSWORD`). A tag release fails if they're missing. Manual dry runs use
  them when they're set. Pull requests never get them, so their dry runs are
  debug-signed.
- **Local and CI builds use Android's standard debug key** `~/.android/debug.keystore`
  (created if missing). Builds from different worktrees then install over each other.
- **CI builds the APK on every push and pull request** (the `android` job) and keeps it as
  a 14-day artifact.
- **iOS isn't released.** CI builds it on every pull request: an unsigned device `.ipa`
  that anyone can re-sign with their own Apple ID, and a simulator app. Both are kept as
  workflow artifacts, not release downloads.

## Alternatives considered
- **A fresh key for every release:** each upgrade would fail with
  `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, forcing users to uninstall and lose app data.
- **A debug keystore committed to the repo:** upgrades would work, but anyone could sign
  an APK that installs as an update over Urushi.
- **Google Play App Signing / an AAB:** needs a Play developer account and Gradle-style
  bundle tooling, for a remote client distributed by direct download.
- **Publishing the unsigned `.ipa` in releases:** users would have to re-sign it with
  their own Apple ID every 7 days (free accounts). Release downloads should install as
  they are, so the `.ipa` stays a CI artifact for testers. Revisit when there's a
  developer account (TestFlight).

## Consequences
- The release keystore must be created once and kept safe outside the repo. Losing it means
  users have to uninstall to move to an APK signed with a new key.
  [CI and releases](../features/ci-and-releases.md) has the commands.
- Releasing no longer needs a manual edit of `AndroidManifest.xml`. Bumping `Cargo.toml` is
  enough.
- `versionCode` jumped from the hand-set `2` to `1001` (0.1.1). It only grows, so existing
  installs upgrade. The scheme allows up to 999 minor and patch versions.
- The release now takes as long as the slower of the two builds.
