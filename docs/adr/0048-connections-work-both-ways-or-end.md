# 0048. Make a connection work both ways or end: a client heartbeat, writers that hang up, ack timeouts, listeners that never stop the daemon

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
UR-69: with many threads open, the UI lost its connection to the daemon, or the daemon froze or crashed. The cause was not one bug but several missing guarantees:

- **The daemon stopped itself when it ran out of file descriptors.** A daemon launched by the macOS app inherits a soft limit of 256 open files. Each pane holds about three (its PTY, and a reader and writer onto it), so about 75 panes, plus the pipes of `git`, `curl` and `ps`, used them up. The local socket's accept loop returned the error (`accept()?`), which ended `run`, and that closed every terminal. The tailnet loop spun on the error without pausing.
- **A connection could die in one direction without anyone noticing.**
  - The daemon's writer task stopped on any write error, including a frame over 8 MiB, without logging anything and without closing the socket. From then on, the client's requests were never answered and its events never came.
  - The client had no timeout and no heartbeat, and counted itself disconnected only when its reader saw the end of the stream. So the UI waited forever instead of reconnecting.
  - On reconnect, the old client reader kept its socket open, which leaked a descriptor on both sides.
- **A dropped `AckScreen` froze its pane for good.** Under load the app drops notifications when its outgoing queue is full. The daemon's screen pump waits for each update's ack, with no limit.

## Decision
A connection either works in both directions or it ends, and nothing about one connection can stop the daemon.

1. **Listeners never stop the daemon.** Every accept loop (local socket, named pipe, tailnet, LAN) logs a failed accept, waits 100 ms and carries on (`server::accept::accept_forever`). Only failing to bind is fatal.
2. **The daemon raises its open-file limit at startup** to the hard limit, capped at 10 240 (macOS refuses more), through `rustix`'s safe `setrlimit`. It logs the limit before and after.
3. **A daemon writer that can't write ends its connection.** `serve` waits on the writer task, so a failed write closes the socket and the client sees the connection end. A frame too large to send is not a write failure. It is logged and dropped; if it was a response, the request gets an `INTERNAL` error instead, so its caller isn't left waiting.
4. **The client checks that the daemon is alive.** A new `Ping` request (protocol 1.27) is answered at once with `Ok`. The client sends one after every 5 s without a frame from the daemon. After **30 s** with no frame at all, it ends the connection: requests in flight fail with `Disconnected`, and the events stream ends, which is the app's cue to reconnect. A daemon older than 1.27 answers `Ping` with an error, which proves it is alive just as well.
5. **The client ends the connection whenever any of its parts stops**: its writer fails, its reader ends, the liveness check fails, or every `Client` handle is dropped. Ending closes the socket, so reconnecting never leaves the old one open. Connecting and the handshake time out after 10 s, and the daemon gives a new connection 10 s to say `Hello`.
6. **A screen update's ack is waited for at most 2 s.** After that it counts as acknowledged. The next update is still a correct diff, because the pump diffs from the last frame it *sent*. A merely slow client still gets at most one update every 2 s, so flow control holds.

## Alternatives considered
- **A timeout on every request.** Rejected: legitimate requests take very different times. Creating a thread fetches from a remote, and deleting one pushes branch deletions; those can take tens of seconds, while `FocusPane` takes microseconds. One timeout would be too short for some and too long for others, and a slow answer is not a dead connection. A heartbeat measures the connection itself.
- **TCP keepalive.** Rejected: it doesn't exist for Unix sockets or named pipes, it takes minutes by default, and it only proves the peer's kernel is alive. A daemon whose writer task died passes every keepalive.
- **Reusing an existing request (`GetSettings`) as the heartbeat.** Rejected: it does real work on every beat, and its purpose would be invisible in the protocol.
- **Making acks reliable on the client** (never dropping an `AckScreen`). Rejected as the only fix: it has to hold for every client, including old mobile builds already installed. A daemon-side timeout repairs a lost ack whoever lost it. Clients still should not drop acks.
- **Writing `setrlimit` ourselves through `libc`.** Rejected: it needs `unsafe`, which is denied workspace-wide. `rustix` was already in the tree, through `alacritty_terminal` and `tempfile`.
- **Fewer descriptors per pane** (sharing one fd for reading and writing). Rejected for now: portable-pty owns those descriptors, and even a third fewer only moves the limit. Raising the limit removes it.

## Consequences
- A daemon that hangs for 30 s, or a network path that stops carrying data, now costs a reconnect, which re-attaches every pane. That is deliberate: before, it cost a frozen UI.
- A pane whose ack was lost stalls for up to 2 s instead of forever.
- An oversized `WorkspaceChanged` (a board near 8 MiB) is now dropped and logged instead of silently killing the connection. The client then misses that change until the next one. Sending deltas instead of whole workspaces is a follow-up.
- New constants to keep consistent: `PING_INTERVAL` (5 s), `LIVENESS_TIMEOUT` (30 s) and `CONNECT_TIMEOUT` (10 s) in `urushi-client`; `ACK_TIMEOUT` (2 s), `HANDSHAKE_TIMEOUT` (10 s) and `ACCEPT_RETRY` (100 ms) in the daemon. `LIVENESS_TIMEOUT` must stay well above `PING_INTERVAL`.
- `rustix` (with the `process` feature) is a direct dependency of the daemon on Unix.
