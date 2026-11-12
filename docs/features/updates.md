# Feature: App updates from GitHub Releases

- **Status:** Shipped (macOS)
- **Owners (crates):** `urushi-daemon` (`update/`, `service/updates.rs`), `urushi-app` (`updates.rs`, `update/updates.rs`, `view/updates.rs`), `urushi-cli` (`update.rs`), `urushi-core` (`Version`, `UpdateSettings`)
- **Related ADRs:** [0048](../adr/0048-update-the-macos-app-from-github-releases-in-the-daemon.md), [0032](../adr/0032-tagged-macos-releases.md), [0013](../adr/0013-restore-points-for-panes.md), [0021](../adr/0021-https-through-curl-and-a-private-credentials-file.md)

## What it does
When a newer release of Urushi is published on GitHub, the macOS app shows a notice in
the window's bottom-right corner. **Update…** asks first, saying how many agents are
working (they will stop, are reopened after the restart, and may need to be asked to
continue) and how many terminals close. It then installs the release in place of the app,
restarts the daemon and relaunches. Panes come back from their restore points. **Later**
hides the notice until a newer release appears. Settings → Updates switches automatic
checks and checks by hand.

## How it works
| Step | Where |
|---|---|
| The daemon learns its bundle: `…/X.app/Contents/MacOS/urushid` whose `Info.plist` says `app.urushi.Urushi`. No bundle (a `cargo` build, Linux, Windows) means no updates. | `update::bundle::bundle_of`, `UpdateConfig::system`, `platform::UPDATES_SUPPORTED` |
| 30 s after start, then every 6 h while `Settings.updates.check_automatically` is on, it asks `releases/latest` and picks `Urushi-<v>-macos-<arch>.zip` and its `.sha256`. | `Service::keep_updates_checked`, `update::github` |
| A newer version than the **installed bundle's** `CFBundleShortVersionString` is kept in memory and sent as `Workspace.update` (`AvailableUpdate`). | `Updater::check`, `Service::workspace` |
| The app shows the notice when connected to its local daemon (protocol ≥ 1.27), on macOS, and the version isn't `Settings.updates.skipped_version`. | `AppState::update_notice`, `view/updates.rs::notice` |
| Confirm → `InstallUpdate{version}`: download into `<home>/updates`, check the SHA-256, unpack beside the bundle, check identifier, version, executables and signature, swap with two renames. | `Service::install_update`, `update::install` |
| `UpdateInstalled{bundle_path}` → the app stops the daemon (`Effect::StopDaemon`, which saves restore points), then `Effect::Relaunch`: a detached `sh` waits for the app to exit and `open`s the bundle. It also reconnects, in case relaunching fails. | `update/updates.rs`, `update/daemon.rs`, `platform::relaunch` |
| `urushi update check` / `urushi update install [--yes]` do the same from a terminal and restart the daemon. Then quit and reopen the app. | `crates/urushi-cli/src/update.rs` |

Settings: `check-updates` and `skipped-update` (`urushi settings set …`), `UpdateSettings` on
the wire.

## Invariants
- Only a daemon inside Urushi's own bundle updates, so a dev daemon never touches
  `/Applications` (`bundle_of`; `a_daemon_outside_a_bundle_never_asks_github`).
- A failed download, a checksum mismatch, the wrong identifier or version, a missing
  executable or a broken signature leave the installed app untouched
  (`a_damaged_or_wrong_release_leaves_the_app_alone`).
- Only the release last found can be installed, one install at a time, and only from
  the daemon's own machine (`Updater::begin_install`, `host_only` in `server/handlers.rs`).
- Nothing restarts without the user confirming: only the red button installs (Enter
  doesn't), and an update under way can't be cancelled (`confirming_installs_then_restarts_the_daemon_then_relaunches`).
- An install that fails stops nothing and asks again with the reason
  (`a_failed_install_asks_again_with_the_reason_and_stops_nothing`).

## Tests
- `crates/urushi-core/src/version.rs`: parsing and numeric order.
- `crates/urushi-daemon/src/update/tests.rs`: release parsing, `Info.plist`, which release
  is offered, install and its failures with the real `ditto`, `codesign` and `shasum`
  (skipped where they're missing).
- `crates/urushi-daemon/tests/updates.rs`: over the protocol with a fake GitHub and a temp
  bundle.
- `crates/urushi-app/src/update/tests/updates.rs` and `view/updates.rs`: the notice, Later,
  the confirmation, install → restart → relaunch, failures, Settings.

## Limitations and follow-ups
- macOS only, and only Apple Silicon builds are published. Mobile apps are installed from
  their own packages; Linux and Windows have no releases.
- The checksum is downloaded from the same release, so it guards against a damaged
  download, not a compromised GitHub account. Developer ID signing and notarization
  would let the updater check the publisher.
- A daemon older than protocol 1.27 can't update: restart it once after installing the
  first version with updates.
- `/Applications` must be writable by the user. If it isn't, the error says so and links
  the release.
- An agent interrupted mid-turn is reopened on its conversation, but the turn is lost.
