# 0043. Remember the remote hosts a device connected to in a plain file, and go back to the last one at launch

- **Status:** Accepted
- **Date:** 2026-11-11

## Context
The mobile apps are remote-only clients. They started with an empty **Connect to a host** dialog on every launch. A LAN host stayed in the dialog because pairing writes the LAN keyring (ADR 0041). A Tailscale host was never saved, so its name had to be typed again after every restart (UR-54). The user wants the app to remember the hosts and to go back to the one they used last.

## Decision
- **One file per device:** `recent-hosts.json` in the client's data directory, next to `lan-keyring.json` (`urushi_client::RecentHosts`). It holds up to 8 addresses (`host[:port]`, as connected to), most recent first. It is written atomically with mode 0600, like the keyring.
- **Written by the backend on a successful connection** to a remote target, once the workspace has loaded, so `update()` stays free of IO. A host that never answered is not remembered. Tailnet and LAN hosts both count.
- **At launch,** when the shell has no host to use (`ConnectTarget::Ask`), the app connects to the first entry without asking (`AppState::resuming`). `URUSHI_HOST` on iOS still wins.
- **When that host can't be reached,** the first failure opens the connect dialog with the host filled in and the reason shown. Nothing has focus, so the keyboard doesn't cover the list. Retries continue underneath. If they get through, the dialog closes. A host picked in the dialog replaces the target.
- **The dialog offers every known host once:** recent hosts first, then paired LAN hosts not used yet. A paired host shows its name; a tailnet host is labelled "Tailscale".

## Alternatives considered
- **The OS keychain (iOS Keychain, Android Keystore):** the file holds no secrets. A tailnet connection needs none (the daemon checks Tailscale identity, ADR 0005), and LAN keys stay in the keyring. Keychain access would need platform code in each shell for no gain.
- **Saving the host inside the LAN keyring:** mixes non-secret history into a secret file, and a damaged keyring would then lose both.
- **Always showing the dialog, with the hosts listed (no auto-connect):** one extra tap on every launch. The user chose auto-connect.
- **Remembering in `update()` when the user submits the dialog:** that would remember hosts that never answered, and would need IO in `update()`.

## Consequences
- A phone launches straight into the last workspace. When that host is gone, it asks within one connection attempt.
- There is no "forget host" control in the UI yet (`RecentHosts::forget` exists). A stale host leaves the list after 8 newer ones.
- The selected thread and tabs are still not remembered for remote hosts (`UiState::path` is local-only).
