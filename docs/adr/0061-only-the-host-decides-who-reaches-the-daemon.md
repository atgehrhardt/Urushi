# 0061. Only the host decides who can reach the daemon, and withdrawn access closes live connections

- **Status:** Accepted
- **Date:** 2026-11-20

## Context
ADR 0041 made LAN access host-only: only a client on the daemon's own socket may turn it on, pair devices or revoke them. Revoking a device or turning LAN access off ends its live connections. Tailnet access (ADR 0005) had neither rule, as the UR-57 security audit found:

- Any remote client (an allowed tailnet login, or a paired LAN device) could send `SetTailnetServe` or a `SettingsPatch` changing the tailnet port, the login allowlist or `allow-tagged-nodes`. A paired phone could therefore open the daemon to every login on the tailnet, or to every tagged server. That contradicted the comment on `host_only`.
- Tailnet identities were checked only when a connection opened. Turning serving off or narrowing the allowlist left sessions already open running, and each one is a shell on the host.

## Decision
- **Who may reach the daemon is decided on the host.** The tailnet patch fields (`tailnet_serve`, `tailnet_port`, `set_allowed_logins`/`allowed_logins`, `allow_tagged_nodes`) and `SetTailnetServe` are refused with `PERMISSION_DENIED` unless they come from the local socket or pipe (`host_only`, `changes_tailnet_access` in `server/handlers.rs`). That matches LAN access. The app shows the tailnet switch only when connected to its own host (`AppState::shows_tailnet_switch`).
- **Access is checked again whenever it changes.** `RemoteState` keeps each admitted tailnet connection's identity next to the sender of a `watch` channel. The connection holds the receiving end (`Peer.close`), the same mechanism LAN uses. Every reconcile (each settings change, and the 30 s watchdog) runs `authorize` again on those identities and drops the senders of the ones no longer allowed (`RemoteState::recheck`). Turning serving off drops them all (`end_all`).
- **Admission is atomic with the check.** `RemoteState::admit` authorizes and registers a connection under the state's lock, reading the settings while holding it. A settings change can't slip between the two.
- **Identification is bounded.** At most 8 connections wait for `tailscale whois` at once (`MAX_PENDING_IDENTIFICATIONS`). Any more are dropped, like LAN handshakes.

## Alternatives considered
- **Leave tailnet settings open to remote clients, since a remote client can type into a terminal anyway.** A terminal lets it act as the user. It shouldn't let it quietly grant a third party a session that shows up nowhere in the host's settings UI, and that outlives the device's own access. A process in a pane can still reach the local socket. This rule is about the protocol's remote surface, not a sandbox.
- **Re-check identities on every request.** That costs a `whois` subprocess per request, and it still wouldn't end an idle connection. Re-checking when the settings change catches every change the daemon makes itself. The 30 s watchdog catches the tailnet owner changing.
- **Close every tailnet connection on any tailnet settings change.** Simpler, but it would end the owner's own phone session whenever an allowlist entry for someone else is added.

## Consequences
- A phone can no longer switch tailnet serving from its Settings; the switch is shown only on the host. Older apps that still send it get `PERMISSION_DENIED` with a reason.
- `RemoteState` now owns live connections as well as listeners, and must prune them when they end. `accept_loop` removes its entry after `serve` returns.
- The wire format is unchanged. The proto comments on `SettingsPatch` fields 5–9 and `SetTailnetServeRequest` say they are host-only.
