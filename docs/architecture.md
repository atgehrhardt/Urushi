# Architecture

## Product shape

Urushi is a **graphical application that contains terminals**. The GUI has:

- a sidebar of **Projects → Threads** (projects group threads from any repo), and a **Repos** tab
- a tab bar, and a status bar under the terminals (branch, and the running agent's plan usage)
- a **Tasks** panel: kanban boards that slide out over the terminals
- draggable split dividers
- buttons, menus, modal dialogs and a settings window

Terminals are one kind of panel inside that GUI. More panel kinds (agent status, diffs, files) will come later.

The multiplexer is **invisible infrastructure**. It exists so terminal sessions are *recoverable*: processes and screen state survive window close, app crash and client disconnect. It has no prefix keys, status bar or text UI.

## Components

```
 Clients (Rust): desktop app (macOS/Linux/Windows) · iOS · Android · `urushi` CLI
   urushi-app     screens + Elm-style update/view (platform-neutral)
   urushi-ui      widget toolkit → Scene (display list)
   urushi-render  wgpu: quads, glyph atlas, backdrop blur
   urushi-client  protocol client, state mirror, transports
        │  versioned protobuf frames (docs/protocol.md)
        │  Unix socket (macOS/Linux) | named pipe (Windows) | TCP on tailnet IP (remote)
        │  | Noise-encrypted TCP on a private LAN address (paired devices, ADR 0041)
 urushid (Rust; macOS/Linux/Windows) ← single source of truth
   workspace model   Repo → Thread → Tab → Layout → Pane; Quick terminal → Layout → Pane; Project groups Threads; Boards of Tasks
   mux               per-pane Session actor: PTY ⇄ VT emulator → damage → diffs
   git               worktree + branch lifecycle via the `git` CLI
   agents            finds installed agent CLIs; tabs type their command into a shell (ADR 0009)
   usage             which agent runs in a pane, and its plan usage (ADR 0011)
   store             JSON document (ADR 0008): repos, projects, threads, layouts, boards, settings
   restore           per-pane restore points: output, directory, agent conversation or running program (ADR 0013)
   attachments       files attached to board tasks, and image thumbnails as raw RGBA (ADR 0016)
   jira              boards that follow Jira Cloud: polling sync (watched boards every 3 s), edits
                     written through, issues read in full on demand, pull requests of issues
                     linked to threads from Jira's development info (ADR 0020, 0022, 0023, 0036)
   http              HTTPS through the system's curl, request on stdin (ADR 0021)
   update            new releases of the macOS app from GitHub: checks, then installs in place
                     of the bundle on request; the app restarts the daemon and itself (ADR 0048)
   remote            Tailscale provider (status/whois) + tailnet listener
   lan               private-address listener, pairing codes, paired-device keys (ADR 0041)
```

## Crates and dependency direction

Dependencies point downwards only. Lower layers never know about higher ones.

```
urushi-app ─► urushi-ui ─► urushi-render          (GUI clients)
     └──────► urushi-client ─► urushi-proto ─► urushi-core
urushi-daemon ─► urushi-mux, urushi-proto, urushi-core
urushi-mux ─► urushi-core                           (the attention signal's format)
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
| **Repo** | A git repository the user added. (`Project` on the v1 wire, ADR 0010.) |
| **Project** | A user-made group of threads, from any repos. It can name a **parent branch** in one repo, which its new threads start from, and keep a **parent-branch thread** working on that branch (ADR 0029). (`Folder` on the wire.) |
| **Thread** | One unit of agent work. **Always** owns exactly one git worktree and its primary branch (named by the user, or `urushi/<slug>-<short-id>`). |
| **Tab** | A layout tree of panes inside a thread. |
| **Quick terminal** | At most one per daemon: a layout of panes in **no** thread, shown as a drop-down panel over the workspace (ADR 0031, [quick terminal](features/quick-terminal.md)). Its id is a tab id, so the tab layout requests act on it; it ends when its last pane closes. |
| **Layout** | An n-ary tree. Leaves are panes. Splits have an axis and child ratios that sum to 1. |
| **Board** | A kanban board of **tasks** in columns (To do, In progress, Done). A task's **subtasks** are tasks nested inside it (ADR 0015), a task can have a kind (task, story, bug, epic) and files attached (kept by the daemon, ADR 0016), and any task can name the thread started for it (ADR 0012) and, on a local board, its pull request (ADR 0036). A board can follow a Jira board (`Board.source`): Jira is then its source of truth, and changes are written to Jira first (ADR 0020); its epics are cards too (ADR 0027). Only organization: it never touches git or terminals. |
| **Pane** | One terminal session. Its shell starts in the thread's worktree (a quick-terminal pane: the home directory, without the thread variables), with `URUSHI_THREAD_ID`, `URUSHI_PANE_ID`, `URUSHI_WORKTREE`, `URUSHI_TTY` (its terminal device), the agent hook settings, `URUSHI_HOME`, the `urushi` CLI's absolute path in `URUSHI_CLI` and its directory first on `PATH` (ADR 0024, ADR 0033) in its environment; values the daemon inherited from a pane it was started in are cleared first (`service/spawn.rs`). A pane whose agent finished or waits for the user carries **attention**, and a pane whose agent is working is **running**. Both are kept in the daemon's memory ([agent attention and activity](features/agent-attention.md)). A tab opened for an agent (Claude, Codex, Cursor) is the same, with the agent's command typed into the shell. |

The pure model lives in `urushi-core`. The daemon persists it and applies changes.

## Terminal data flow

1. The PTY produces bytes. The Session actor feeds them to the emulator (`alacritty_terminal` behind the `Emulator` trait) and records which rows changed.
2. Updates are batched to at most ~120 Hz. Each attached client gets only the rows that changed, as styled spans, sent by its own subscriber.
3. **Flow control:** at most one un-acknowledged update per client per pane. A slow client gets one larger, merged update rather than an unbounded queue. This matters for mobile and remote links.
4. Input travels as *abstract* events (key + modifiers, text, paste, mouse). The daemon turns them into escape sequences, because it knows the terminal's current modes (application cursor keys, bracketed paste, kitty keyboard protocol).
5. **Reattach** = a full snapshot, then live diffs. Because the screen state lives in the daemon, a new client sees exactly what was on screen.

## Thread lifecycle

- **Create** (all-or-nothing):
  1. Resolve the base. With `update_base_from_remote` (default on), fetch the base's branch from its remote and start from the remote's commit if the local branch is only behind it. The user's checkout is never touched, and a failed fetch falls back to the local commit (ADR 0028, `service/fresh_base.rs`). Then snapshot the existing branches.
  2. `git worktree add -b <branch> <data>/worktrees/<repo>/<thread> <base>`
  3. Persist the thread.
  4. Spawn the first tab, unless the client asked for none (the GUI then shows the agent launcher). A thread started from a task with `start_task_agent` runs the user's task agent there, with its prompt in `URUSHI_TASK_PROMPT` (ADR 0014): a reference to the task and `<CLI's absolute path> board show` to read it (ADR 0033), or a copy of it without a CLI (ADR 0024), or the user's own message filled in for the task; that agent and prompt are resolved before step 2. When the user drafts task messages, the prompt is left off the command line and `service/task_draft.rs` pastes it into the agent, unsent, once the agent is ready (ADR 0030).

  A task can also start in an existing thread: `CreateTab{task_id, start_task_agent}` opens a tab running the task agent there and links the task in the same mutation (ADR 0018).

  If any step fails, everything is rolled back.

  Without a base, a thread starts from its project's parent branch (if that is in the same repo), else the repo's current branch.
- **Parent-branch thread** (ADR 0029, `service/project_base.rs`): it works on its project's parent branch. If a worktree already has that branch checked out (usually the repo's own folder), it runs there and is *attached*. Otherwise it gets `git worktree add <path> <branch>` without `-b`. It owns no branch. Closing it is deleting it through the same two steps: an attached thread's plan has `keeps_worktree`, so nothing on disk is removed. Deleting it switches it off in the project's settings.
- **Ownership:** a thread owns its primary branch, plus branches that did not exist at creation and were checked out in its worktree (read from the worktree's own HEAD reflog). It never owns the base branch, the default branch, branches that existed before it, or branches checked out in other worktrees.
- **Delete** (two steps, enforced by the protocol):
  1. `PlanThreadDeletion` returns a `DeletionPlan`: the worktree, dirty/untracked counts, unpushed commits, local branches, remote branches (when `delete_remote_branches`, default **on**) and running processes. It also returns a token.
  2. The client **always** shows a warning modal.
  3. `DeleteThread(token)` re-computes the plan and refuses if anything changed. Then it kills the sessions, removes the worktree, deletes local branches, deletes remote branches and prunes.

  If a step fails partway, the thread is marked `DeletionFailed`, which can be retried.

## Remote access (Tailscale, LAN)

- The daemon uses the **installed** Tailscale client (through its CLI, behind the `TailscaleProvider` trait).
- With "Serve on tailnet" enabled, it binds only on its tailnet IPs and re-binds when they change.
- Every connection is authorized with `whois` against an allowlist. The default allows only the host owner's login, and tagged nodes are denied.
- WireGuard already encrypts the traffic.
- **LAN access** (ADR 0041, off by default) is the other way in. The daemon binds only this machine's private addresses (`lan::addresses`, re-checked every 30 s by `server::lan`) and never touches a router or firewall. Connections run the protocol inside a Noise channel (`urushi_proto::lan`). A device must first pair with a one-time code the host shows; after that, only paired keys are answered. Changing LAN access, pairing and revoking are allowed only over the local socket (`handlers::host_only`). Revoking a device closes its connections, through `Peer.close`.
- Local IPC is authorized by OS user:
  - On Unix, the socket is mode 0600 inside a 0700 home, and the peer's user id is checked.
  - The socket lives at `<home>/urushid.sock`. Homes too long for a socket path use `~/.urushi/sockets/<hash>.sock` instead.
  - On Windows, a named pipe that rejects remote clients. Its name is a hash of the home path's text (`urushi_proto::endpoint::local_endpoint`).
  - The daemon derives its endpoint from the home exactly as it was given (the spelling clients use), before canonicalizing the home for everything else. Otherwise a Windows client would look for a different pipe. One daemon per home is still enforced by the lock file inside the home.

## Platform strategy

- Platform-specific code appears only in `platform/` modules: data dirs, IPC transport, daemon launching, clipboard, file picker, mobile soft keyboard.
- Desktop entry points use winit. iOS and Android are thin shells around `urushi-app`. They don't run a local daemon.

## Dependencies on external programs

- `git` and `tailscale` are run as separate processes, never linked. So are `ps` (which agent runs in a pane), `curl` (Claude's usage endpoint, ADR 0011; Jira Cloud's REST APIs, ADR 0021; GitHub's releases, ADR 0048) and, on macOS, `security` (Claude's keychain sign-in; see ADR 0011) and `shasum`, `ditto` and `codesign` (checking and installing an update, ADR 0048).
- The Jira sign-in (site, email, API token) is kept by the daemon in `<home>/jira-credentials.json` (0600) and never sent to clients (ADR 0021).
- Their absence is reported in the UI with a clear explanation.
