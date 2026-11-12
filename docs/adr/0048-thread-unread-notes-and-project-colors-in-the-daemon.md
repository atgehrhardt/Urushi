# 0048. Keep a thread's unread mark and notes, and a project's color, in the daemon

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
UR-62 ("Threads Bar") asks for three things in the sidebar:
- marking a thread as unread (UR-63),
- leaving notes on a thread (UR-64),
- coloring a project's section (UR-65).

The sidebar already has state of two kinds:
- **Workspace state**, kept by the daemon and shared by every client: projects, pins, statuses and the thread order (ADR 0010, ADR 0038).
- **Per-client view state**, kept only in the app: collapsed sections, the open menu, drags.

There is also in-memory agent attention (ADR 0017). It is "finished / needs input" from the agent, and it clears once the pane is on screen.

The same workspace is open on a desktop, a phone and a remote machine at once. All three must look the same there and survive a daemon restart.

## Decision
- **All three are workspace state, stored by the daemon** (ADR 0002):
  - `Thread.unread` (a flag)
  - `Thread.notes` (plain text, at most 10 000 characters, trailing whitespace dropped)
  - `Project.color` (protocol 1.27)
  The two thread fields are set through `UpdateThread`, like pins and statuses, so one update stays all or nothing.
- **Unread is a manual mark, separate from attention.** The user marks a thread unread (or read) from its menu. The **client that opens the thread** clears the mark by sending `UpdateThread{unread: false}` when the user selects the thread. Marking the thread that is already open leaves it unread until it is opened again, as mail clients do.
- **Project colors use the board palette** (`LabelColor`, 9 named colors). Each client picks the shade for its theme. The color is set with its own request, `SetFolderColor{folder_id, color}`.

## Alternatives considered
- **Unread and notes as per-client view state** (like collapsed sections). This is simpler and needs no protocol change, but a thread marked unread on the desktop wouldn't show as unread on the phone, and notes would be lost with the app's config. Notes, at least, are clearly data.
- **The daemon clears unread when the user types or clicks in one of the thread's panes** (as attention does). That would miss a user who only reads the output, and only the client knows which thread is open. Selecting the thread is the act of reading it.
- **Marking threads unread automatically when an agent finishes.** Attention already does that and clears once the pane is seen. A second automatic signal would duplicate it. The user asked for a manual mark.
- **Free RGB colors for projects.** These look wrong in one of the themes and need a picker. The board's named palette already solves both.
- **The color as part of `FolderSettings`.** `UpdateFolderSettings` replaces all of a project's settings, and it can refuse (`FAILED_PRECONDITION`) because of the parent-branch thread. Changing a color shouldn't depend on that, and a client would have to resend settings it doesn't edit. A separate request mirrors `UpdateColumn`'s color.

## Consequences
- Every client shows the same unread threads, notes and colors, and they survive restarts. Older clients ignore the new fields.
- Opening an unread thread costs one request, and only when the thread is unread.
- Two clients opening the same thread both send the clear. It is idempotent.
- Notes are plain text: no formatting and no history.
- Older daemons (< 1.27) get none of the menu items, and the app doesn't send the requests.
