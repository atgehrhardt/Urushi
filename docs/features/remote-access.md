# Feature: Remote access over Tailscale

- **Status:** Shipped in the daemon and CLI. The GUI status pill and host picker are pending.
- **Owners (crates):** `urushi-daemon` (`tailscale.rs`, `server/remote.rs`), `urushi-client`, `urushi-cli`
- **Related ADRs:** [0005](../adr/0005-tailscale-remote.md)

## What it does
With `tailnet.serve` on (`urushi tailnet on`, or the app's toggle), the daemon is reachable from your other devices at `<machine>.<tailnet>.ts.net:7420`. That includes the mobile apps and `urushi --host`. Only you can connect by default.

## How it works
- `TailscaleProvider` (`CliTailscale`) reads `tailscale status --json` (this machine's addresses, MagicDNS name, owner login, peers) and `tailscale whois --json <ip>`.
- `server::remote::reconcile` binds one TCP listener per tailnet address, never `0.0.0.0`. It runs when settings change, and every 30 s from a watchdog, so changed addresses get re-bound.
- **Each connection is identified before the handshake** and checked by `authorize`:
  - Tagged devices are refused unless `allow-tagged-nodes`.
  - With an allowlist, the login must be on it.
  - Otherwise the login must match the machine's owner.
- Traffic is already encrypted by WireGuard, so the protocol is the same as local.

## Invariants
- Never listens on non-tailnet interfaces.
- An unknown owner login denies everyone.

## Tests
- `server/remote.rs` unit tests: the authorization rules.
- `tailscale.rs`: JSON parsing.
- `tests/e2e.rs`: serving admits the owner over TCP and refuses another user (using a `StaticTailscale` fake).

## Limitations and follow-ups
- Requires the Tailscale app or daemon on both ends. An embedded `tsnet` sidecar is a possible future option (see the ADR).
- The CLI-based provider spawns a process per check. Tailscale's local API would be cheaper.
