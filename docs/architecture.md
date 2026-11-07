# Architecture

## Product shape

Urushi is a **graphical application that contains terminals**. The GUI has:

- a sidebar of **Projects → Threads**
- a tab bar
- draggable split dividers
- buttons, menus, modal dialogs and a settings window

Terminals are one kind of panel inside that GUI. More panel kinds (agent status, diffs, files) will come later.

The multiplexer is **invisible infrastructure**. It exists so terminal sessions are *recoverable*: processes and screen state survive window close, app crash and client disconnect. It has no prefix keys, status bar or text UI.

## Components

```
 Clients (Rust): desktop app (macOS/Linux/Windows) · iOS · Android · `urushi` CLI
   urushi-app     screens + Elm-style update/view (platform-neutral)
   urushi-ui      widget toolkit → Scene (display list)
   urushi-render  wgpu: quads, glyph atlas, instanced terminal grid
   urushi-client  protocol client, state mirror, transports
        │  versioned protobuf frames (docs/protocol.md)
        │  Unix socket (macOS/Linux) | named pipe (Windows) | TCP on tailnet IP (remote)
 urushid (Rust; macOS/Linux/Windows) ← single source of truth
   workspace model   Project → Thread → Tab → Layout → Pane
   mux               per-pane Session actor: PTY ⇄ VT emulator → damage → diffs
   git               worktree + branch lifecycle via the `git` CLI
   store             JSON document (ADR 0008): projects, threads, layouts, settings
   remote            Tailscale provider (status/whois) + tailnet listener
```

## Crates and dependency direction

Dependencies point downwards only. Lower layers never know about higher ones.

```
urushi-app ─► urushi-ui ─► urushi-render          (GUI clients)
     └──────► urushi-client ─► urushi-proto ─► urushi-core
urushi-daemon ─► urushi-mux, urushi-proto, urushi-core
urushi-cli ─► urushi-client
```

| Crate | Responsibility |
|---|---|
| `urushi-core` | Pure domain model and rules. No IO. |
| `urushi-proto` | Wire schema types, framing, validated conversions to and from core. |
| `urushi-mux` | PTYs, VT emulation, per-pane session actors, key encoding. |
| `urushi-daemon` | The `urushid` binary: server, workspace service, git, store, Tailscale. |
| `urushi-client` | Connecting, the handshake, request/response matching, the state mirror. |
| `urushi-cli` | The `urushi` binary for scripting and headless verification. |
| `urushi-ui` | Widget toolkit (Elm-style) → `Scene`. |
| `urushi-render` | wgpu renderer for a `Scene`. |
| `urushi-app` | The GUI application (screens, dialogs, settings). |

## Data model

| Concept | Meaning |
|---|---|
| **Project** | A git repository the user added. |
| **Thread** | One unit of agent work. **Always** owns exactly one git worktree and its primary branch `urushi/<slug>-<short-id>`. |
| **Tab** | A layout tree of panes inside a thread. |
| **Layout** | An n-ary tree. Leaves are panes. Splits have an axis and child ratios that sum to 1. |
| **Pane** | One terminal session. Its shell starts in the thread's worktree, with `URUSHI_THREAD_ID` and `URUSHI_WORKTREE` set in its environment. |

The pure model lives in `urushi-core`. The daemon persists it and applies changes.

## Terminal data flow

1. The PTY produces bytes. The Session actor feeds them to the emulator (`alacritty_terminal` behind the `Emulator` trait) and records which rows changed.
2. Updates are batched to at most ~120 Hz. Each attached client gets only the rows that changed, as styled spans, sent by its own subscriber.
3. **Flow control:** at most one un-acknowledged update per client per pane. A slow client gets one larger, merged update rather than an unbounded queue. This matters for mobile and remote links.
4. Input travels as *abstract* events (key + modifiers, text, paste, mouse). The daemon turns them into escape sequences, because it knows the terminal's current modes (application cursor keys, bracketed paste, kitty keyboard protocol).
5. **Reattach** = a full snapshot, then live diffs. Because the screen state lives in the daemon, a new client sees exactly what was on screen.

## Thread lifecycle

- **Create** (all-or-nothing):
  1. Snapshot the existing branches.
  2. `git worktree add -b <branch> <data>/worktrees/<project>/<thread> <base>`
  3. Persist the thread.
  4. Spawn the first tab.

  If any step fails, everything is rolled back.
- **Ownership:** a thread owns its primary branch, plus branches that did not exist at creation and were checked out in its worktree (read from the worktree's own HEAD reflog). It never owns the base branch, the default branch, branches that existed before it, or branches checked out in other worktrees.
- **Delete** (two steps, enforced by the protocol):
  1. `PlanThreadDeletion` returns a `DeletionPlan`: the worktree, dirty/untracked counts, unpushed commits, local branches, remote branches (when `delete_remote_branches`, default **on**) and running processes. It also returns a token.
  2. The client **always** shows a warning modal.
  3. `DeleteThread(token)` re-computes the plan and refuses if anything changed. Then it kills the sessions, removes the worktree, deletes local branches, deletes remote branches and prunes.

  If a step fails partway, the thread is marked `DeletionFailed`, which can be retried.

## Remote access (Tailscale)

- The daemon uses the **installed** Tailscale client (through its CLI, behind the `TailscaleProvider` trait).
- With "Serve on tailnet" enabled, it binds only on its tailnet IPs and re-binds when they change.
- Every connection is authorized with `whois` against an allowlist. The default allows only the host owner's login, and tagged nodes are denied.
- WireGuard already encrypts the traffic.
- Local IPC is authorized by OS user:
  - On Unix, the socket is mode 0600 inside a 0700 home, and the peer's user id is checked.
  - The socket lives at `<home>/urushid.sock`. Homes too long for a socket path use `~/.urushi/sockets/<hash>.sock` instead.
  - On Windows, a named pipe that rejects remote clients.

## Platform strategy

- Platform-specific code appears only in `platform/` modules: data dirs, IPC transport, daemon launching, clipboard, file picker, mobile soft keyboard.
- Desktop entry points use winit. iOS and Android are thin shells around `urushi-app`. They don't run a local daemon.

## Dependencies on external programs

- `git` and `tailscale` are run as separate processes, never linked.
- Their absence is reported in the UI with a clear explanation.
