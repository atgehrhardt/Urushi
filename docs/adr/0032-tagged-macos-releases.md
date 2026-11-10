# 0032. Release macOS builds from a version tag, for Apple Silicon, as an ad-hoc signed zip

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Until now the only way to get Urushi was to build it from source (`make install`).
UR-36 asks for a GitHub Actions workflow that publishes a release, starting with macOS
only. We had to choose what starts a release, which Macs it supports, and how it is
signed and packaged. There's no Apple Developer account wired into the repo, and the
bundle is already assembled and ad-hoc signed by `scripts/bundle-macos.sh`.

## Decision
- `.github/workflows/release.yml` runs when a tag `vX.Y.Z` is pushed. It fails unless the
  tag equals `[workspace.package].version` in `Cargo.toml`, so the app's
  `CFBundleShortVersionString` always matches the release.
- It builds on `macos-latest` (arm64) with the same `scripts/bundle-macos.sh` that
  `make app` uses. It checks the bundle (three binaries, arm64, `codesign --verify`),
  zips it with `ditto -c -k --keepParent`, and publishes
  `Urushi-<version>-macos-arm64.zip` plus its `.sha256` with `gh release create`.
- Running the workflow by hand (`workflow_dispatch`), or a pull request that changes
  `release.yml` or `scripts/bundle-macos.sh`, is a dry run: it builds the zip as a
  workflow artifact and publishes nothing.

## Alternatives considered
- **A manual button that creates the tag:** a release could then be made from any
  commit with a typed version that might not match `Cargo.toml`. Tagging is the usual git
  habit, and the version check makes a mismatch impossible.
- **A rolling nightly from every push to `main`:** nobody needs it yet, and it would
  publish untested intermediate states as downloads.
- **A universal (arm64 + x86_64) binary:** roughly doubles the build time, for Intel Macs
  nobody has asked for. It can be added later with a second target and `lipo`.
- **Developer ID signing and notarization:** these need a paid Apple Developer account and
  certificate secrets in the repo. Without them, a notarized build is impossible.
- **A `.dmg`:** adds a packaging step and gives no signing benefit over a zip while the app
  isn't notarized.

## Consequences
- Releasing is a matter of bumping the version, committing, then
  `git tag vX.Y.Z && git push origin vX.Y.Z`.
- Users must allow the first launch (right-click → Open, or
  `xattr -dr com.apple.quarantine`), because the app is ad-hoc signed. The release notes
  say so.
- Only Apple Silicon Macs are supported. Windows and Linux have no release builds.
- The release job doesn't re-run the tests. It trusts CI on the tagged commit, so tags
  should only be put on commits where CI is green.
