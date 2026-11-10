# 0030. Draft a task's message by pasting it into the agent once it says it is ready

- **Status:** Accepted
- **Date:** 2026-11-10
- **Amends:** [0014](0014-task-prompt-via-pane-environment.md) (how the message reaches the
  agent when the user drafts instead of sends)

## Context
A task's agent starts with the task's message as its first prompt (ADR 0014, ADR 0024).
Claude, Codex and Cursor submit that prompt at once. Users want a setting to have the
message only **drafted**, waiting in the agent's input for them to edit and send (UR-30).
They also want to replace the default message with their own.

None of the three CLIs can start with unsent text in its input. Each takes a prompt
argument, and each sends it.

So the only way to draft is to type the message into the agent after it starts. ADR 0014
rejected that for sending, because the daemon must know when the agent reads input. That
moment differs per agent. Each agent also shows startup screens that ask for bracketed
paste just as its input does. Tried against the real CLIs:
- Claude's folder-trust dialog has "No, exit" selected by default.
- Codex shows an "Update now?" menu, then a trust dialog.

## Decision
- Two settings in the daemon, separate from each other:
  - `AgentSettings.task_delivery`: **Send** (the default) or **Draft**.
  - `AgentSettings.task_message`: the user's own message and whether it is on. Off by
    default. `{key}`, `{title}`, `{url}` and `{show}` are filled in for each task
    (`board::custom_task_prompt`).

  These are protocol 1.19 fields.
- **Drafting:** `"$URUSHI_TASK_PROMPT"` is left off the command line. The prompt stays in
  the pane's environment. `service/task_draft.rs` pastes the prompt, bracketed and without
  Enter, once the agent is ready:
  - **Claude:** Urushi's hook settings add a `SessionStart` hook (matcher `startup`).
    It writes a new **`ESC]777;urushi;ready BEL`** signal to the pane's terminal. The mux
    counts these signals in `SessionStatus.ready`; they are never attention.
    `SessionStart` runs only after Claude's startup dialogs are answered. This was checked
    in an untrusted directory: no signal while the trust dialog showed, then the signal
    0.15 s after it was accepted.
  - **Codex and Cursor:** Urushi can't add a hook. Codex runs only hooks the user has
    trusted, and Cursor has none on its command line. Two conditions together count as
    ready:
    - The pane has bracketed paste on.
    - A process-table check shows the agent itself is the shell's foreground job. This
      rules out the shell's own prompt, which also turns on bracketed paste.
  - In both cases the screen must then stay unchanged for 500 ms.
- Before the agent is ready, the user's keys answer its startup dialogs (or the shell)
  and leave the draft alone. The draft is dropped, with a log line, in three cases. The
  thread and tab stay as they are:
  - The user types or clicks in the pane once the agent is ready
    (`Service::user_acted`). Pasting then would mix the draft into what they type.
  - The pane's program exits.
  - Nothing is ready in time: 2 min for Claude, so the user can answer its dialogs, and
    30 s otherwise.

## Alternatives considered
- **Wait a fixed delay, then paste:** too short and the shell gets the text, too long and
  the user is already typing. Neither ever knows about a dialog.
- **Bracketed paste and a still screen alone:** this was the first plan. Claude's trust
  dialog and Codex's update menu both pass it.
- **Match agent-specific text on the screen** (Codex's "Ask Codex to do anything"):
  this breaks with every agent release and every language.
- **Type the command line into the shell without Enter:** this drafts the *command*, not
  the message. The user would still be sending the prompt as an argument.
- **Have the client paste:** this duplicates the readiness logic in every client
  (ADR 0002). It also needs a client attached at the right moment.

## Consequences
- Claude drafts never land in its startup dialogs. A Codex or Cursor draft can still meet
  one. A paste into Codex's menus was tried and is ignored, so the draft is lost but
  nothing is chosen.
- Every Claude started by Urushi now runs one more hook, `SessionStart` on startup.
- The draft lives only in the daemon's memory. A daemon restart before the agent is ready
  loses it.
- Claude shows a long paste as `[Pasted text]` until edited. The default message is short
  enough to show in full.
- If the agent's CLI gains a "prefill" flag, it should replace pasting for that agent.
