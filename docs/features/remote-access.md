# Feature: Remote access over Tailscale

- **Status:** Shipped in the daemon, the CLI and the mobile apps (connect dialog, remembered hosts). The desktop GUI's status pill is pending.
- **Owners (crates):** `urushi-daemon` (`tailscale.rs`, `server/remote.rs`), `urushi-client` (`hosts.rs`), `urushi-app` (`update/lan.rs`, `view/lan.rs`), `urushi-cli`
- **Related ADRs:** [0005](../adr/0005-tailscale-remote.md), [0043](../adr/0043-mobile-clients-remember-recent-hosts.md)
- **See also:** [LAN access](lan-access.md), for reaching the daemon without Tailscale.

## What it does
With `tailnet.serve` on (`urushi tailnet on`, or the app's toggle), the daemon is reachable from your other devices at `<machine>.<tailnet>.ts.net:7420`. That includes the mobile apps and `urushi --host`. Only you can connect by default.

## How it works
- `TailscaleProvider` (`CliTailscale`) reads `tailscale status --json` (this machine's addresses, MagicDNS name, owner login, peers) and `tailscale whois --json <ip>`.
- The CLI runs with `TAILSCALE_BE_CLI=1`. On macOS the CLI is usually the Tailscale.app binary itself. Started without a terminal, as when Urushi.app launches the daemon, it tries to open its GUI and prints an error with exit code 0. If the output still isn't JSON, the error shows what the CLI printed, and `reconcile` logs it to `urushid.log` when it changes.
- `server::remote::reconcile` binds one TCP listener per tailnet address, never `0.0.0.0`. It runs when settings change, and every 30 s from a watchdog, so changed addresses get re-bound.
- **Each connection is identified before the handshake** and checked by `authorize`:
  - Tagged devices are refused unless `allow-tagged-nodes`.
  - With an allowlist, the login must be on it.
  - Otherwise the login must match the machine's owner.
- Traffic is already encrypted by WireGuard, so the protocol is the same as local.
- **Remembered hosts (the apps, ADR 0043):**
  - After a remote connection succeeds, the app's backend records the address in `recent-hosts.json` (`urushi_client::RecentHosts`) in its data dir. The file is 0600 and holds up to 8 addresses, most recent first. It contains no secrets.
  - At launch with no host chosen (`ConnectTarget::Ask`, the phones), the app goes back to the first entry without asking (`AppState::resuming`).
  - If that fails, the first failure opens the connect dialog with the host filled in and the reason shown (`resume_failed`). Retries continue, and the dialog closes if they get through (`resume_succeeded`).
  - The connect dialog lists recent hosts, then paired LAN hosts not used yet, each once (`update::lan::offered_hosts`). One tap connects.

## Invariants
- Never listens on non-tailnet interfaces.
- An unknown owner login denies everyone.

## Tests
- `server/remote.rs` unit tests: the authorization rules.
- `tailscale.rs`: JSON parsing.
- `tests/tailscale_cli.rs`: `CliTailscale` against fake binaries, including one that acts like the macOS app binary without a terminal.
- `tests/e2e.rs`: serving admits the owner over TCP and refuses another user (using a `StaticTailscale` fake).
- `crates/urushi-client/src/hosts.rs`: the recent hosts file. Covers nothing written before a connection, 0600, most recent first without duplicates, the cap, forgetting, and a damaged file being an error.
- `crates/urushi-app/src/update/tests/hosts.rs`: going back to the last host, asking once when it fails and closing when retries get through, the dialog starting from the current host, and the offered list. `view/lan.rs`: a recent host connects with one tap.

## Limitations and follow-ups
- Requires the Tailscale app or daemon on both ends. Without it, use [LAN access](lan-access.md). An embedded `tsnet` sidecar is a possible future option (see the ADR).
- The CLI-based provider spawns a process per check. Tailscale's local API would be cheaper.
- The apps can't forget a recent host from the UI yet. A host drops off the list after 8 newer ones.
