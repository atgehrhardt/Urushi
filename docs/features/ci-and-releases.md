# Feature: CI and releases

- **Status:** Shipped (CI on every push and pull request; macOS releases from tags)
- **Owners (crates):** none; `.github/workflows/`, `scripts/bundle-macos.sh`, `Makefile`
- **Related ADRs:** [0032](../adr/0032-tagged-macos-releases.md), [0007](../adr/0007-platform-isolation.md)

## What it does
Every push to `main` and every pull request runs the same checks as `make ci`, on macOS,
Linux and Windows. Pushing a tag `vX.Y.Z` builds `Urushi.app` for Apple Silicon and
publishes it as a GitHub Release.

## How it works
**CI** (`.github/workflows/ci.yml`):

| Job | Runs on | What |
|---|---|---|
| `test (<os>)` | macOS, Ubuntu, Windows | `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --all-targets`, doc tests |
| `mobile-targets` | macOS | `make check-targets` (iOS and Android client crates) |
| `docs` | Ubuntu | `scripts/check-docs.sh` |
| `licenses` | Ubuntu | `cargo deny check` |
| `proto` | Ubuntu | `buf lint`; on pull requests, also `buf breaking` against `main` |

The toolchain comes from `rust-toolchain.toml`. The daemon's end-to-end tests
(`crates/urushi-daemon/tests/*.rs`) are Unix-only, because they drive `/bin/sh` and git
fixtures. On Windows the unit tests and the `cmd.exe` PTY test run.

**Releases** (`.github/workflows/release.yml`, ADR 0032):
1. Bump `[workspace.package].version` in `Cargo.toml` and commit it. Wait for CI to be green.
2. Run `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The workflow checks that the tag matches the version, then runs `scripts/bundle-macos.sh`
   (release profile, ad-hoc `codesign`). It checks the bundle and uploads
   `Urushi-X.Y.Z-macos-arm64.zip` and its `.sha256` to a new release, with install notes
   and generated change notes.

**Dry run:** Actions → Release → *Run workflow* builds the same zip as a workflow artifact
and publishes nothing.

## Invariants
- A release's version, its tag and the app's `CFBundleShortVersionString` are equal: the
  workflow's *Version* step refuses a mismatched tag.
- Releases are built by the same script as `make app` and `make install`, so a released
  app is laid out exactly like a local one.
- `#[cfg(target_os/unix/windows)]` stays inside `platform/` modules (ADR 0007). CI
  compiles all three desktop OSes, which catches Unix-only APIs used outside them (e.g.
  `MasterPty::tty_name`, now `urushi_mux::platform::tty_path`).
- Daemon integration tests don't depend on timing: the test helpers report a daemon that
  failed to start with its actual error (`tests/common/mod.rs`). Transient states are
  observed by holding the fake service in place (`FakeJira::hold_transitions`), not by
  racing it.

## Tests
- The workflows themselves are exercised by running them: CI on the pull request, and the
  release workflow via a manual dry run.
- Before pushing, `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc`
  (and `x86_64-unknown-linux-gnu`) type-checks the other desktop OSes from a Mac.

## Limitations and follow-ups
- The app isn't notarized. Users allow the first launch by hand. Notarization needs an
  Apple Developer ID certificate and notary credentials as repository secrets.
- Only Apple Silicon is released. Intel Macs would need an `x86_64-apple-darwin` build
  merged with `lipo`.
- There are no Windows or Linux releases, and no iOS or Android store builds.
- The release job doesn't re-run the tests. Tag only commits where CI passed.
