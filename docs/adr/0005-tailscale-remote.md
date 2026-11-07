# 0005. Remote access via the installed Tailscale client

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
Remote and mobile clients need secure access to a daemon. We want native Tailscale integration, or at least an easy way to make sure the daemon is reachable on the tailnet.

## Decision
- A `TailscaleProvider` trait with `status()` and `whois()`, implemented first on top of the `tailscale` CLI's JSON output.
- When "Serve on tailnet" is on, the daemon binds TCP only on its tailnet IPs (never `0.0.0.0`) and re-binds when they change.
- Every connection is authorized with `whois` against an allowlist. The default allows only the host owner's login, and tagged nodes are denied.
- WireGuard provides encryption.

## Alternatives considered
- **Embedding `tsnet` (Go, BSD-3) as a sidecar:** the daemon would join the tailnet as its own device without Tailscale installed. But it adds a Go toolchain to the build and a second binary to ship. We can revisit it behind the trait.
- **`tailscale serve` TCP forwarding:** connections would arrive from localhost, losing the peer's identity.

## Consequences
- Tailscale must be installed and logged in on the host and the client devices. The UI shows the status and actionable errors.
