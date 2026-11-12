# 0049. Update the macOS app from GitHub Releases in the daemon, by swapping the bundle and restarting

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
UR-67 asks that a new GitHub Release show a notice in the app's bottom-right corner, and
that clicking it update the app after the user accepts that running agents will be
stopped, restored after the restart, and may need restarting if they were mid-task.

Releases already publish `Urushi-X.Y.Z-macos-arm64.zip` and its `.sha256` from a tag
(ADR 0032). The app is ad-hoc signed, not notarized, so there is no Sparkle-style signed
feed. Installing (`make install`) swaps the bundle with renames and never restarts the
daemon, because the daemon owns every terminal. Restore points (ADR 0013) already bring
panes, programs and agent conversations back after a restart. The user chose: macOS only,
automatic checks with a Settings switch, and "Later" skipping a release until a newer
one appears.

## Decision
- **The daemon checks and installs** (`urushi_daemon::update`). It reads
  `GET api.github.com/repos/atgehrhardt/Urushi/releases/latest` through `HttpClient`
  (curl, ADR 0021), 30 s after start and every 6 h while `Settings.updates.check_automatically`
  is on, and on demand (`CheckForUpdates`). It compares the release with the version in
  the **installed bundle's** `Info.plist`, not its own binary's, and tells clients through
  `Workspace.update` (in memory, like attention).
- **Only a daemon running from Urushi's own macOS bundle updates**
  (`…/X.app/Contents/MacOS/urushid` with identifier `app.urushi.Urushi`). Development
  builds and other OSes refuse, so a dev daemon can never touch `/Applications`.
- **Installing (`InstallUpdate`, local clients only)**: download the zip and its
  checksum into `<home>/updates`, check the SHA-256 (`shasum`), unpack (`ditto`) beside
  the bundle, check the identifier, the version, the three executables and the signature
  (`codesign --verify --deep --strict`), then swap with two renames, as
  `install-macos.sh` does. Any failure leaves the installed app untouched. It doesn't
  restart anything.
- **The app restarts**: it offers the release in a corner notice, confirms with the
  number of working agents and open terminals, sends `InstallUpdate`, then stops the
  daemon (`ShutdownDaemon`, which saves restore points), and relaunches itself from the
  bundle (a detached `sh` waits for the app to exit, then runs `open`). The new app
  starts the new daemon. `urushi update install` does the same from the CLI.
- "Later" stores the version in `Settings.updates.skipped_version`.

## Alternatives considered
- **The app checks and installs on its own.** It has no HTTP client (that is the
  daemon's, ADR 0021), every window would poll GitHub, the CLI couldn't share it, and
  daemon tests couldn't fake it. The app still restarts itself, since only it can.
- **Sparkle or another updater framework.** Objective-C, needs an EdDSA-signed appcast
  and is built around notarized apps; it doesn't know our daemon must be restarted on
  purpose. Its MIT licence is fine, but it would be our first non-Rust runtime piece.
- **A helper process that installs while the app and daemon are stopped.** Unneeded: on
  macOS, renaming a bundle away keeps running binaries working, which `make install` has
  relied on since the start.
- **Compare with the daemon's own version.** After `make install` the running daemon is
  older than the app on disk, and would offer an update that's already installed.
- **Verify a signature instead of a checksum.** The app is ad-hoc signed, which proves
  integrity but not origin. The checksum comes over HTTPS from the same release, so it
  guards against a damaged download, not a compromised GitHub account; Developer ID
  signing and notarization would close that (see the CI follow-ups).
- **Restart the daemon without asking.** Stopping it ends every running agent; the user
  must agree, as with Settings → Restart daemon….

## Consequences
- Release asset names (`Urushi-<version>-macos-<arch>.zip` and `.sha256`) and the tag
  format are a contract with the updater; `releases/latest` must not be a draft or a
  pre-release.
- A daemon from before protocol 1.28 can't update. The user restarts it once after
  installing the first version with updates.
- The daemon calls GitHub's API without credentials (60 requests an hour per address);
  checking every 6 h is far below that.
- `/Applications` must be writable by the user. Otherwise the error says so and links the
  release page.
- An agent interrupted mid-task is reopened, but the turn it was in is lost. The
  confirmation says so.
