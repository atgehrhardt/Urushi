# 0013. Bring panes back after a daemon restart from per-pane restore points

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
Processes die when the daemon stops: on **Restart daemon…** (to switch to a new
version), on a reboot or logout, or in a crash. Until now the next daemon restored
threads, tabs and layouts with fresh shells in each worktree. Everything else was lost:
what the panes showed, where their shells had `cd`'d to, and the agent conversations
running in them, which are what users care about most. The user asked for every thread's
panes to come back as they were.

A process's memory can't be kept across a restart without keeping the process (a live
upgrade that hands the PTYs to the new daemon, see the recoverable-sessions follow-up).
That doesn't help after a reboot or crash anyway. What can be brought back is what the
daemon can observe and the programs themselves can reopen.

## Decision
- **The daemon keeps a restore point per pane:** `<home>/restore/<pane id>.json`
  (`urushi_daemon::restore`), with the terminal size, the working directory, the agent
  running in it and its conversation, or else the job in the terminal's foreground with
  each process's exact arguments, and its last 2,000 lines of output as terminal bytes
  (`urushi_mux::replay`). It's brought up to date every 15 s, and on a
  clean shutdown before the processes are killed, so a crash loses at most 15 s. Output
  is read again only when the screen changed. A full-screen app (`vim`, `less`) keeps
  the output from before it. Restore points of closed panes are deleted.
- **Restoring a shell pane:** the new emulator is fed the saved output and a dim
  "restored" line before the new shell starts, in the saved directory (the worktree if
  that's gone).
- **Restoring a pane that was running a program** (`vim`, `less`, a dev server, a
  pipeline): the shell starts in the job's directory and the job's command line is typed
  and run, as if the user pressed Enter. The user chose this over typing it without
  Enter: panes should come back as they were. The job is the shell's children in the
  terminal's foreground process group (`ps` `pgid`/`tpgid`), so background jobs and an
  idle prompt don't count. Arguments come from the OS via `sysinfo`, exactly (`ps` joins
  them with spaces), cut at a control character where a program overwrote its own (BSD
  `yes`); a program that rewrote its title into one argument (`npm run dev`) is split
  back into words. Each word is POSIX-quoted (`urushi_core::shell`).
- **Typing waits for the prompt:** a restored pane's command is typed once its shell has
  printed and gone quiet for 0.5 s (at most 15 s), because some shell setups discard
  input typed while they start.
- **Restoring an agent pane:** the shell starts in the saved directory and the agent's
  resume command is typed into it, as starting an agent is (ADR 0009). Claude Code names
  its open session in `~/.claude/sessions/<pid>.json`, so its exact conversation is
  resumed (`claude --resume <id>`); a session with no transcript yet starts a fresh
  Claude. Codex and Cursor don't say, so they resume their latest conversation in that
  directory (`codex resume --last`, `cursor-agent --continue`). The skip-permissions
  setting applies as when starting. Session ids are validated (`AgentSessionId`) before
  being typed into a shell.
- **The window's own selection** (which thread and tabs it shows) is remembered by the
  app, not the daemon, for the local daemon (`urushi_app::ui_state`).

This changes one consequence of ADR 0009: a restart now restores an agent pane as the
agent, not as a plain shell. Typing commands into a shell is unchanged.

## Alternatives considered
- **Keep restore data in the workspace document** (ADR 0008): it's rewritten and
  broadcast on every change, and output would make it large. Restore points change
  constantly and no client needs them.
- **Save only on shutdown:** a crash, a kill or a power cut would lose everything.
- **Type the program's command but let the user press Enter** (or re-run only a list of
  known-safe programs): safer for an interrupted deploy or migration, but panes wouldn't
  come back as they were, and a list would never be complete. The user chose re-running.
- **Read arguments from `ps`, or from the OS through our own FFI:** `ps` loses the
  boundaries between arguments (a path with a space becomes two). Reading
  `KERN_PROCARGS2` ourselves would bring the first `unsafe` into the codebase; `sysinfo`
  (MIT, its dependencies already in the tree) does it on macOS, Linux and Windows, and
  gives working directories too, replacing an `lsof` call.
- **`claude --continue` for Claude too:** with two Claude panes in one worktree both
  would reopen the same conversation. The session file gives each pane its own.
- **Find Codex sessions by matching its logs to processes** (cwd and start time): more
  code and still a guess, for a gain only when two Codex panes share a directory.
- **Shell integration (OSC 7) for the working directory:** needs every user's shell
  configured. Asking the OS (`sysinfo`) works with any shell.

## Consequences
- After a restart, reboot or crash, panes show their old output and shells are back in
  their directories; agents reopen their conversations and programs run again.
- A job that was interrupted mid-way (a deploy, a migration) starts again from the
  beginning. Restarting the daemon already warns that whatever runs in the terminals
  stops.
- Pane output is written to disk in the daemon's home (owner-only, like the rest of it),
  and deleted when the pane closes.
- The daemon depends on details the agent CLIs don't promise: Claude's session files,
  and each agent's resume flags. If they change, restore falls back to a resume of the
  latest conversation, or an error the user sees in the pane, and the shell remains.
- Every 15 s the daemon runs `ps` and asks the OS about each pane's processes. Windows
  has no `ps`, so it restores output and working directories, but not agents or
  programs.
