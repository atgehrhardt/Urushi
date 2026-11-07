# 0002. The daemon is the single source of truth; clients are thin

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
We need desktop clients on three operating systems, mobile clients that can't spawn processes, remote access, and sessions that outlive the app.

## Decision
`urushid` owns everything stateful:

- workspace model, layouts and settings
- PTYs and terminal emulation
- input encoding
- git operations

Clients mirror state they receive and send abstract intents ("split pane", "key Ctrl+C").

## Alternatives considered
- **Emulating the terminal in the client and sending raw bytes:** would mean two emulators that can drift apart, a reattach snapshot that still has to come from the server, and emulation duplicated in every client.
- **No daemon (the app owns the PTYs):** sessions die with the app, and mobile and remote clients become impossible.

## Consequences
- Every new client is cheap: it renders a grid and sends input.
- Latency adds one local socket hop, which is negligible (the same model as tmux).
- The protocol becomes a core, long-lived interface, so it needs care (ADR 0003).
