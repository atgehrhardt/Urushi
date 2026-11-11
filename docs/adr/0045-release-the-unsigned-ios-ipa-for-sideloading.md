# 0045. Release the unsigned iOS .ipa from the same tag, for users to sideload

- **Status:** Accepted
- **Date:** 2026-11-11
- **Supersedes:** the "iOS isn't released" part of [0040](0040-release-android-apks-signed-with-a-repository-secret-key.md)

## Context
ADR [0040](0040-release-android-apks-signed-with-a-repository-secret-key.md) released
Android but kept the iOS `.ipa` out of releases: without a paid Apple Developer account,
the only device build is unsigned. Users have to re-sign it with their own Apple ID before
an iPhone runs it, and with a free Apple ID they have to re-sign it every 7 days. The
`.ipa` was only a 14-day CI artifact, which testers outside the repo can't easily find and
which disappears after 14 days.

The user asked for the iOS build in every release, with instructions for sideloading it
(UR-50). There's still no developer account.

## Decision
- **Every `vX.Y.Z` release includes the unsigned device build.** The `ios` job in
  `release.yml` runs `scripts/bundle-ios.sh --device` (the same script as `make ios` and CI)
  on `macos-latest`. It checks the bundle (an arm64 executable, `CFBundleShortVersionString`
  equal to the release version, the bundle identifier), then uploads
  `Urushi-<version>-ios-arm64-unsigned.ipa` with its `.sha256`.
- **The file name says "unsigned"**, so nobody expects it to install as it is.
- **The release notes explain how to sideload it:** a re-signing tool (Sideloadly, AltStore,
  or Xcode with a paid account), trusting the Apple ID on the phone, Developer Mode, the
  7-day limit and the 3-app limit for free accounts, and upgrading with the same tool and
  Apple ID.
- **`publish` waits for the iOS job too**, so a release still contains every platform or
  none.
- The simulator build stays a CI artifact. It's for developers, not users.

## Alternatives considered
- **Keep it a CI artifact only (ADR 0040):** users can't find it from the release page, and
  it expires after 14 days.
- **Ad-hoc sign it:** devices don't run ad-hoc signatures (only the simulator does), and
  re-signing tools replace any signature anyway.
- **TestFlight or the App Store:** both need a paid Apple Developer account. Revisit when
  there is one. A signed build would then replace the unsigned one.

## Consequences
- Each iOS user signs the app themselves. With a free Apple ID, they have to re-sign it
  every 7 days.
- Releases run a second macOS job, which costs more runner minutes than Linux, but it runs
  in parallel, so the release takes about as long as before.
- A broken iOS build now blocks the whole release, as a broken Android build already does.
  CI's `ios` job builds the same `.ipa` on every pull request, so it should fail there first.
