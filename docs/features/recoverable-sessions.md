# Feature: Recoverable terminal sessions

- **Status:** Shipped (daemon, client, CLI and GUI), including restore after a daemon restart
- **Owners (crates):** `urushi-mux` (`replay.rs`), `urushi-daemon` (`restore/`, `service/restore.rs`), `urushi-core` (`agent/resume.rs`), `urushi-client`, `urushi-app` (`ui_state.rs`)
- **Related ADRs:** [0001](../adr/0001-in-house-multiplexer.md), [0002](../adr/0002-daemon-source-of-truth.md), [0013](../adr/0013-restore-points-for-panes.md)

## What it does
Every pane is a real process on a PTY, owned by the daemon. Closing the window, the app crashing or the network dropping doesn't stop the process. Reattaching shows exactly what was on screen.

When the daemon itself stops (**Restart daemon…**, a reboot, a crash), the processes end, but every pane of every ready thread, and of the quick terminal, comes back as it was:
- **Shells** show their last 2,000 lines of output, then a dim "── restored after the daemon restarted ──" line, and a new shell in the directory the old one was in.
- **Agents** start again on the same conversation: Claude resumes the exact session the pane had open (a Claude that had nothing said yet starts fresh), and Codex and Cursor resume their latest conversation in that directory.
- **Programs** that were running in the foreground (`vim notes.md`, `less`, `npm run dev`, `tail -f log | grep x`) run again with the same arguments, in the directory they ran in.
- **Reopening the app** shows the thread and tabs it showed when it was closed (local daemon).

## How it works
- **`SessionHandle::spawn`** starts the process (`Pty` trait, `NativePty` via portable-pty) and an actor task that alone owns the `Emulator` (`AlacrittyEmulator`).
- **Blocking PTY I/O runs on dedicated threads.** The queues between those threads and the actor are bounded.
- **Line versions:** after each output chunk, the actor turns the emulator's damage into per-line versions. Watchers are notified through `watch<SessionStatus>`.
- **Output is batched:** the actor takes every output chunk already queued (up to 1 MiB, `session/output.rs`) before it records damage and bumps the version once, so a busy pane wakes its watchers once per batch.
- **Clients pull changes:** a client asks for `frame_since(version)` and gets only the lines changed since then (a `full` frame after a resize or full damage). Combined with the protocol's ack rule (one un-acknowledged `ScreenUpdate` per pane), slow clients get merged updates instead of growing queues. The daemon waits at most 2 s for an ack (`server/attach.rs::ACK_TIMEOUT`), so a lost one only delays the pane (ADR 0048).
- **Ending a process** (closing a pane, deleting a thread): the actor hands the process to a thread of its own, which signals it (escalating to SIGKILL) and reaps it (`session.rs::kill_and_reap`). No runtime worker sleeps, and no zombie is left.
- **Many panes:** every pane holds about three file descriptors, so the daemon raises its open-file limit at startup (to the hard limit, at most 10 240; logged as "open file limit"). Without that, a daemon started by the macOS app stops at 256, which is about 75 panes.
- **One size per pane:** a session has one terminal size, the last one asked for (`AttachPane` or `SetPaneSize`, from any client). Every attached client gets the full frame that follows, at the new size. Which client asks is the GUI's choice (ADR 0044).
- **Input encoding happens in the daemon** (`keys::encode_key`, `encode_paste`, `mouse::encode_mouse`), because it depends on terminal modes such as application cursor keys, bracketed paste and mouse tracking.
- **Mouse** (`urushi-mux/src/mouse.rs`): reports follow the application's tracking mode (1000 click, 1002 drag, 1003 any motion) and encoding (SGR 1006, UTF-8 1005, or X10, which can't report beyond column/row 223). **The wheel** (`mouse::wheel`) becomes mouse reports if the app tracks the mouse, arrow keys on the alternate screen (1007, on by default), and otherwise scrolls the scrollback. Full-screen agents such as Claude Code use the alternate screen with SGR tracking, so they scroll their own transcript.

## Upgrading the daemon
- Installing never replaces a running daemon, because stopping it closes every terminal. The user restarts it on purpose: **Settings → Restart daemon…** (the button is highlighted when the daemon is older than the app), or `urushi daemon restart` (`urushi daemon stop` to just stop it). `make install` prints the command.
- `urushi_client::stop_local_daemon` sends `ShutdownDaemon` to daemons from protocol 1.4. Older daemons get SIGTERM: the pid comes from the socket's peer credentials, so it is exactly the daemon for that home, never a process-name match. It then waits until the process has exited (a zombie counts as exited). Reconnecting starts the daemon bundled with the client (`launcher::daemon_binary`, which also finds `Contents/MacOS/urushid` from the bundled CLI).
- The next daemon restores threads, tabs and layouts, and each pane from its restore point (below).

## Restore points (ADR 0013)
- **What's saved** (`urushi_daemon::restore::PaneRecord`, `<home>/restore/<pane id>.json`): terminal size; the working directory; the agent under the shell (`usage::process::agent_process_under`) and its `Conversation` (`Saved(id)`, `Empty` or `Unknown`; Claude's from `~/.claude/sessions/<pid>.json` and its transcript in `projects/`, `restore/sessions.rs`); or else the foreground `job` (`process::foreground_job`: the shell's children whose process group is the terminal's foreground group, from `ps -o pid,ppid,pgid,tpgid,args`), each stage's exact arguments from `UsageSource::details` (`sysinfo`, cut at control characters a program wrote into its own arguments); and the output (`SessionHandle::snapshot` → `replay::to_ansi`: text with SGR styles, control characters dropped). With a job, the directory is the job's (`cd x && make` runs in `x`), else the shell's.
- **When:** every 15 s (`Service::keep_restore_points`, with a fresh process table) and on shutdown before the processes are killed (`run` → `Service::save_restore_points`, up to 5 s). Output is read only from panes whose screen version changed; on the alternate screen (`vim`, `less`, a full-screen agent) the previous output is kept. An agent pane saves no output: the agent redraws its conversation when it resumes. Files of panes that no longer exist are deleted.
- **Restoring** (`service/restore.rs::restore_sessions`): the emulator is created at the saved size and fed the saved output plus the separator line before the shell spawns (`spawn_session_in`, in the saved directory if it still exists, else the worktree). Then, once the shell has printed its prompt and been quiet for 0.5 s (`wait_for_prompt`, at most 15 s; typing earlier is lost with some zsh setups), one of these is typed and run: for an agent, `AgentRecord::start` picks `Agent::resume_command_line` (`claude --resume <id>` / `--continue`, `codex resume <id>` / `--last`, `cursor-agent --resume <id>` / `--continue`) or, for an empty conversation, the normal command line; for a job, `shell::job_command_line` (each argument POSIX-quoted, stages joined by ` | `, a rewritten title like `npm run dev` split back into words). A missing agent leaves a plain shell.
- **The app** remembers the selected thread, each thread's tab, and the sidebar's width and collapse in `<home>/app-state` (`ui_state.rs`), saved when they change and applied before the first workspace arrives; threads deleted meanwhile fall back to the first.

## Invariants
- Only the actor touches the emulator: no locks, and no shared mutable state.
- A frame requested from before the last reset is always `full`.
- Pastes can't break out of bracketed-paste mode (embedded end markers are stripped).
- A session id only reaches a shell if it is letters, digits, `-` and `_` (`AgentSessionId::new`); a job's arguments are quoted, and a job with a control character in an argument isn't typed (`shell::job_command_line`); replayed output carries no control characters (`replay::to_ansi`).
- A missing, corrupt or unreadable restore point never stops a pane from starting: it starts as a plain shell in its worktree (`RestoreStore::load`).
- A stopping daemon never records its own kills as panes exiting. `Service::shutdown` freezes the workspace before it kills the sessions: `on_pane_exited` does nothing, and `Service::mutate` refuses every change (`Unavailable`, "the daemon is stopping"), so the stored layout keeps every pane for the next daemon. Every local and tailnet connection hangs up (`connection::serve` waits on `Service::stopped`), so no socket or pipe of the old daemon lingers. On Windows a lingering pipe instance would keep the next daemon from creating its pipe. This matters most when the daemon runs inside a longer-lived process (the tests), where tasks outlive `run`.

## Tests
- `crates/urushi-mux/src/emulator/alacritty.rs`: spans, wide characters, damage, replies, modes, scrollback.
- `crates/urushi-mux/src/keys.rs` and `mouse.rs`: the encoding tables and wheel decisions.
- `crates/urushi-mux/src/replay.rs`: replayed output looks the same (colours, styles, wide characters, scrollback), control characters dropped.
- `crates/urushi-core/src/agent/resume.rs`: resume command lines per agent, session id validation. `shell.rs`: quoting, pipelines, rewritten titles, control characters refused.
- `crates/urushi-daemon/src/usage/process.rs`: the foreground job (pipelines, not helpers or background jobs), damaged arguments.
- `crates/urushi-daemon/src/restore/`: records saved, loaded and pruned; Claude session files and transcripts; how an agent starts again.
- `crates/urushi-daemon/tests/quick_terminal.rs`: the quick terminal and its output come back after a restart.
- `crates/urushi-daemon/tests/restore.rs`: a real daemon restarted: old output above the new shell, the shell back in its subdirectory, a (fake) Claude resumed with its session id, a running program started again with an argument containing a space; closed panes' restore points deleted.
- `crates/urushi-app/src/ui_state.rs` and `update/tests.rs::reopening_shows_the_remembered_thread_and_tab`.
- Manually verified against a throwaway daemon home with zsh and Claude Code 2.1: output, colours and directory restored; Claude typed `--resume` with the id from its session file, and an empty session came back as a fresh Claude; `less "my notes.txt"`, `python3 -m http.server` (serving again) and a `yes | head | sleep` pipeline ran again.
- `crates/urushi-daemon/tests/restart.rs`: stopping the real `urushid` by request and by signal, and the next daemon restoring the workspace. `tests/e2e.rs`: `ShutdownDaemon` refused over the tailnet.
- `crates/urushi-daemon/tests/shutdown.rs`: after a stop, the killed shell's pane is still in `workspace.json`, and a client that was connected has been hung up on, without the store being written.
- `crates/urushi-daemon/tests/mouse.rs`: SGR reports reaching a real program, arrow keys on the alternate screen, scrollback in a plain shell, also when output lands after the first frame (the tests ack every update, as clients must; `common::next_screen_update`).
- `crates/urushi-mux/tests/pty_session.rs`: real processes (output, echo, exit codes, resize, incremental frames, env/cwd), and a killed process that ignores SIGHUP is reaped, not left a zombie. `session/output.rs`: queued output taken in batches.
- `crates/urushi-daemon/tests/many_panes.rs`: 100 panes from a daemon started with a 256 open-file limit all work, and new connections are still served. `tests/stability.rs`: a pane whose ack is never sent still updates.
- `crates/urushi-daemon/src/server/accept.rs`: an accept error is retried, never fatal. `server/connection/tests.rs`: an oversized response becomes an error, an oversized event is dropped, a failed write ends the writer.
- `crates/urushi-client/tests/liveness.rs`: a silent daemon ends the connection after the liveness timeout and fails requests in flight; an older daemon answering pings with errors stays connected; dropping every client closes the socket; a missing `Welcome` times out.
- `crates/urushi-daemon/tests/e2e.rs`:
  - screen survives a client disconnect
  - no update is sent before an ack
  - exits close panes
  - a daemon restart restores layouts and respawns shells in the worktree

## Limitations and follow-ups
- The scrollback position is shared by every client attached to a pane, not tracked per client.
- Processes don't survive a daemon *restart*: programs start again from the beginning (an editor's unsaved changes, a server's state, a half-done script's progress are gone), and shell variables are the new shell's. A command's environment prefix (`FOO=1 make`) isn't seen, and a program that rewrote its title to something other than its command (e.g. `next-server`) can't be re-run as typed. The follow-up is a live upgrade: the new daemon inheriting the running PTYs and screens from the old one, so installing can switch daemons without closing terminals.
- Up to 15 s of output and directory changes are lost in a crash or power cut.
- Two Codex or Cursor panes in one directory both resume the latest conversation there; only Claude says which conversation a pane has.
- Claude started from inside another Claude Code session (e.g. a daemon launched from a Claude terminal) inherits `CLAUDE_CODE_CHILD_SESSION`, saves no transcript, and so can't be resumed.
- Replayed lines don't re-wrap like live output when the pane is narrower than when they were saved.
- Windows has no `ps`, so agents and programs aren't restored there (output and directories are).
- The app's remembered selection is per machine, for the local daemon only; remote and mobile clients start on the first thread.
- Stopping a daemon by signal (protocol before 1.4) works on macOS and Linux, not Windows.
- Each pane keeps up to 10 000 lines of scrollback at its full width, so many wide, chatty panes use a lot of memory. Not yet bounded by total size.
- Killing a pane signals its shell, not the shell's whole process group, so a program that left the terminal's session can outlive it.
- The emulator's synchronized-output timeout (DEC 2026, 150 ms) isn't driven by a timer: an application that starts a synchronized update and then goes quiet shows stale content until it writes again.
