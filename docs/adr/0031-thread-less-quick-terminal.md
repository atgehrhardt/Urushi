# 0031. Keep one thread-less quick terminal per daemon, beside the tabs

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Users want a terminal they can summon from anywhere in the app with a button or a
shortcut, for work that belongs to no thread (UR-31): a drop-down "quick terminal".
Hiding it must not stop what runs in it, and it can be split like a tab.

Every pane so far lives in a `Tab`, and a tab must belong to a thread, which must own a
worktree (AGENTS.md invariant 8, ADR 0004). `Tab.thread` is required in the domain
type, in `workspace.json` and on the wire. The daemon relies on it when it repairs the
store at startup (tabs of missing threads are dropped), when it restores panes (only of
ready threads), when it splits a pane (the thread gives the directory and shell) and
when it deletes a thread (its tabs' panes are killed). The layout tree and the terminal
plumbing (attach, input, resize, attention) only use pane ids.

## Decision
- The daemon holds **at most one quick terminal**, shared by every client (desktop and
  mobile), next to the tabs: `State.quick_terminal`, stored as `quick_terminal` in
  `workspace.json` (format 1, absent when none) and sent as `Workspace.quick_terminal`
  (protocol 1.20).
- A tab's layout and focus move into `urushi_core::PaneGroup`. `Tab` owns one;
  `QuickTerminal { id, group }` owns another.
- The quick terminal's id is a `TabId`. The existing layout requests (`SplitPane`,
  `ClosePane`, `FocusPane`, `MoveDivider`, `ResizePaneEdge`, `Equalize`, `CloseTab`) take
  it as their `tab_id`, and the daemon resolves either (`State::group`). The only new
  request is `OpenQuickTerminal`, which returns the open one or opens it with one shell.
- Its panes start in the user's home directory, with the global default shell and no
  `URUSHI_THREAD_ID` or `URUSHI_WORKTREE` (`PaneHome::Quick`). They are restored after a
  restart like thread panes, and raise attention like them.
- It ends when its last pane closes or exits. Hiding the panel is a client matter: the
  client detaches its panes and the processes keep running in the daemon.

## Alternatives considered
- **An optional thread on `Tab`** (or an owner enum): a tab without a thread would be
  representable everywhere tabs are used, every thread-coupled path (repair, restore,
  deletion, the app's thread tab lists) would need a new case, and older clients would
  reject such tabs (`tab_from_proto` requires a thread).
- **A hidden "scratch" thread:** it would need a worktree of some repo (invariant 8), and
  would show up wherever threads are listed.
- **A quick terminal per client:** the daemon is the source of truth (ADR 0002); a
  terminal that follows the user from the desktop to their phone is more useful, and one
  per client would leak processes for clients that never come back.
- **`Tab` on the wire with an empty `thread_id`:** older clients would fail to parse it.
  A separate field is invisible to them.

## Consequences
- Older clients never see the quick terminal; newer clients check
  `Welcome.protocol_minor ≥ 20` before offering it.
- Code that walks every pane (restore points, `urushi ls`, `resolve`) must include the
  quick terminal: `State::all_panes`, `WorkspaceMirror::group` and `owner_of_pane`.
- Downgrading the daemon drops the quick terminal: an older daemon rewrites
  `workspace.json` without the field, and its panes' restore points are deleted.
- More than one quick terminal, or naming them, would need a list on the wire and a new
  ADR.
