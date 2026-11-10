# Wire protocol

> Status: **v1.13 implemented**. The schema lives in [`proto/urushi/v1/`](../proto/urushi/v1/), the codec and conversions in `crates/urushi-proto`.
> Any change must follow the `change-protocol` skill.

## Transport

| Context | Transport | Authorization |
|---|---|---|
| Local, macOS/Linux | Unix domain socket in the runtime dir, mode 0600 | Peer's OS user id must match the daemon's user |
| Local, Windows | Named pipe | DACL limited to the current user |
| Remote | TCP bound **only** to Tailscale IPs | Tailscale `whois` → allowlist of tailnet logins |

The protocol never assumes client and daemon are on the same machine. No file paths are exchanged for content, and there is no shared memory. (Attachments travel as bytes, ADR 0016.)

## Framing

Each frame is `u32` big-endian length, then a protobuf-encoded `Envelope`. Frames have a maximum size (initially 8 MiB). Anything larger closes the connection.

```
Envelope {
  oneof kind {
    Request      { id, oneof body }    // client → daemon, always answered
    Response     { id, oneof outcome } // daemon → client, matches a Request id
    Event        { oneof body }        // daemon → client, unsolicited
    Notification { oneof body }        // client → daemon, fire-and-forget (input, acks, scroll)
  }
}
```

## Handshake

The client sends `Hello { protocol_major, protocol_minor, client_name, client_version }`.
The daemon replies with `Welcome { protocol_major, protocol_minor, daemon_version, host_name }`, or with an error and closes the connection.
Within one major version, minor versions are additive. A different major version is rejected.

| Minor | Adds |
|---|---|
| 0 | Everything below unless marked. |
| 13 | Richer Jira issues and quick filters: `JiraIssue.{type_id, assignee_id, reporter (JiraUser{account_id, name}), priority_id, due, points, sprint (JiraNamed{id, name}), components, fix_versions, links}`, `JiraBoardSource.quick_filters` (`JiraQuickFilter{id, name}`; their JQL stays in the daemon), `JiraAccount.account_id`, and `QueryJiraFilter{board_id, quick_filter_ids}` → `JiraIssueIds{ids}` (the issues every one of those quick filters lets through, as Jira evaluates them; an unknown quick filter is `NOT_FOUND`). Jira issues in full (ADR 0022, `jira_issue.proto`): `GetJiraIssue{task_id}` → `JiraIssueDetails{fields[JiraField{id, name, kind (JiraFieldKind), required, allowed[JiraOption], value (JiraFieldValue{text, options})}], transitions, comments[JiraComment{…, mine}], attachments, links, link_types, watchers, watching, worklogs, original_estimate, remaining_estimate, time_spent}`; `ChangeJiraIssue{task_id, oneof change: edit (JiraFieldEdits), transition_id, add_comment, update_comment, delete_comment_id, attach (≤ 6 MiB), delete_attachment_id, link, unlink_id, watch, log_work}` → `JiraIssueDetails` afterwards (an edit Urushi can't write, e.g. a bad date or a field of kind `OTHER`, is `INVALID_ARGUMENT`); `GetJiraAttachment{task_id, attachment_id, thumbnail}` → `AttachmentData`; `SearchJiraUsers{task_id (may be empty), query, assignable}` → `JiraUsers`; `SearchJiraIssues{board_id, query}` → `JiraIssueRefs`; `GetJiraCreateFields{board_id, kind}` → `JiraIssueDetails` (fields only) and `CreateTask.jira_fields`. Descriptions and comments travel as Markdown. `Settings.appearance` and `SettingsPatch.appearance` (`Appearance{scheme (ColorScheme), custom (SchemeColors{background, glows, accent}, 0xRRGGBB), logo (LogoStyle)}`; unknown values read as the original look; set in a patch, it replaces the look). `WatchBoard{board_id}` (empty: none) says which Jira board a connection shows; the daemon reads it every few seconds until the connection asks for another or closes (ADR 0023). Clients check `Welcome.protocol_minor ≥ 13`. |
| 12 | Task kinds and Jira boards (ADR 0020): the `TaskKind` enum (`UNSPECIFIED` = none; unknown values read as none), `Task.kind`, `Subtask.kind`, `CreateTask.kind`, `UpdateTask.optional kind`. `Board.jira` (`JiraBoardSource{site, board_id, name, project_key, scrum, synced_at_ms, error, dropped, url}`), `Board.epics` (`Epic{key, summary, color, url}`), `BoardColumn.jira_statuses`, `Task.issue` and `Subtask.issue` (`JiraIssue{id, key, url, type_name, status_id, status, category, assignee, priority, epic_key, notes_editable, parent_id, rank, updated_ms}`, the `JiraStatusCategory` enum). On a Jira board, task changes are made in Jira first; what Jira owns (columns, renaming the board, nesting, deleting issues, attachments) is `FAILED_PRECONDITION`. `jira.proto`: `ConnectJira{site, email, api_token}` → `JiraAccount` (`PERMISSION_DENIED` if Jira refuses), `GetJiraAccount` → `JiraAccount`, `DisconnectJira`, `ListJiraBoards{query}` → `JiraBoardList`, `ImportJiraBoard{jira_board_id}` → `Board`, `SyncBoard{board_id}`. `ERROR_CODE_UNAVAILABLE` when Jira can't be reached. The API token is never sent back. Clients check `Welcome.protocol_minor ≥ 12`. |
| 11 | `MoveFolder{folder_id, index}` reorders projects (folders): the index counts positions without the moved folder and is clamped to the end; an unknown folder is `NOT_FOUND`. `Workspace.folders` is in display order. Clients check `Welcome.protocol_minor ≥ 11` before sending it. |
| 10 | Starting a task in an existing thread (ADR 0018): `CreateTab.{task_id, subtask_id, start_task_agent}`, with the same meaning as on `CreateThread`. The task is linked to the tab's thread (moving any earlier link) in the same mutation that records the tab. With `start_task_agent`, the tab runs the task agent with the task as its prompt; this overrides `agent`. Refusals come before any terminal starts: no task agent chosen or not installed is `FAILED_PRECONDITION`, `start_task_agent` without a task is `INVALID_ARGUMENT`, and a missing task is `NOT_FOUND`. Older daemons ignore the fields, so clients check `Welcome.protocol_minor ≥ 10`. |
| 9 | Agent attention (ADR 0017): `Tab.attention` (`PaneAttention{pane_id, kind, since_ms}`, the `AttentionKind` enum: `DONE`, `WAITING`; `UNSPECIFIED` from a newer daemon reads as waiting) lists panes whose agent finished or that wait for the user. It isn't stored: changes arrive as `WorkspaceChanged` with the same `revision`. `ClearAttention{pane_ids}` clears panes the user is looking at (unknown or calm panes are ignored); typing or clicking in a pane clears it too. Clients check `Welcome.protocol_minor ≥ 9` before sending `ClearAttention`. |
| 8 | Board labels and columns: the `LabelColor` enum (`UNSPECIFIED` = no color; unknown values read as none), `BoardColumn.color`, `Task.{color, tags}`, `CreateTask.{color, tags}`, `UpdateTask.{optional color, TagList tags}`, and `CreateColumn` → `BoardColumn`, `UpdateColumn`, `MoveColumn`, `DeleteColumn` (its tasks move to the column on its left, or right for the first; the last column is `FAILED_PRECONDITION`). Subtasks as nested tasks (ADR 0015): `Subtask.{notes, created_at_ms, color, tags, subtasks}`, `NestTask{task_id, parent_id, index}`, and subtask ids (at any depth) in `UpdateTask`, `MoveTask` (onto the board), `DeleteTask`, `CreateSubtask` and `CreateThread.subtask_id`. Attachments (ADR 0016): `Task.attachments`, `Subtask.attachments` (`Attachment{id, name, media_type, size, created_at_ms, width, height}`), `AddAttachment{task_id, name, bytes data}` → `Attachment` (at most 6 MiB; an image the daemon can't decode is `INVALID_ARGUMENT`), `GetAttachment{attachment_id, thumbnail}` → `AttachmentData` (the file, or an image's thumbnail as raw RGBA fitting 320 px; `FAILED_PRECONDITION` for a thumbnail of a non-image), `DeleteAttachment`. Clients check `Welcome.protocol_minor ≥ 8`. |
| 7 | The task agent (ADR 0014): `AgentSettings.{task_agent, task_mode}`, `SettingsPatch.{task_agent, task_mode}`, the `AgentMode` enum (`agents.proto`, which now also holds `Agent`), and `CreateThread.start_task_agent`, which opens the first tab running the task agent with the linked task as its prompt. No task agent chosen or not installed is `FAILED_PRECONDITION`, and no task is `INVALID_ARGUMENT`, before any worktree is made. Clients check `Welcome.protocol_minor ≥ 7`. |
| 6 | Subtasks and threads started from tasks: `Task.{subtasks[Subtask], thread_id}`, `Subtask{id, title, done, thread_id}`, `CreateTask.subtasks` (titles), `CreateSubtask` → `Subtask`, `UpdateSubtask{optional title, optional done}`, `DeleteSubtask`, and `CreateThread.{task_id, subtask_id}`, which links the new thread to the task or subtask (ADR 0012). A missing task is `NOT_FOUND` before any worktree is made; deleting a thread clears its links. Clients check `Welcome.protocol_minor ≥ 6`. |
| 5 | Task boards: `Workspace.boards` (`Board{columns[BoardColumn{tasks[Task]}]}`), `CreateBoard` → `Board`, `RenameBoard`, `DeleteBoard`, `CreateTask` → `Task`, `UpdateTask{optional title, optional notes}`, `MoveTask{column_id, index}`, `DeleteTask`. Agent usage: `GetAgentUsage{pane_id}` → `AgentUsage{agent, plan, windows[UsageWindow], unavailable, fetched_at_ms}` (ADR 0011). Clients check `Welcome.protocol_minor ≥ 5` before sending them. |
| 4 | `ShutdownDaemon` (local connections only; `PERMISSION_DENIED` over the tailnet). Older daemons are stopped with SIGTERM instead, the pid read from the socket's peer credentials. |
| 3 | `Input.mouse` (`MouseInput`: button, press/release/move, cell, modifiers) and `Input.wheel` (`WheelInput`). Clients send `ScrollPane` for the wheel to older daemons. |
| 2 | Projects as `Folder`s: `Workspace.folders`, `Thread.{folder_id, pinned, status}`, `CreateThread.folder_id`, `UpdateThread`, `CreateFolder` → `Folder`, `RenameFolder`, `DeleteFolder`. |
| 1 | `ListAgents` → `Agents`; `CreateTab.agent`; `CreateThread.branch` and `skip_initial_tab`; `Settings.agents` and `SettingsPatch.*_skip_permissions`. Clients check `Welcome.protocol_minor ≥ 1` before sending `ListAgents`. Older daemons ignore the new fields (a plain shell, a generated branch, a first tab). |

## Naming (ADR 0010)
v1 field names are frozen (`buf breaking` uses `WIRE_JSON`). On the wire, **`Project` means repo** (`projects`, `project_id`, `AddProject`, `RemoveProject`, `UpdateProjectSettings`), and **`Folder` means project** (a user-made group of threads). `urushi-proto::convert` maps them to `urushi_core::Repo` and `urushi_core::Project`.

## Message groups (v1)

- **Workspace:** `ListWorkspace{subscribe}`, `AddProject`, `RemoveProject`, `ListBranches`, `CreateThread{branch, skip_initial_tab, folder_id, task_id, subtask_id, start_task_agent}`, `RenameThread`, `UpdateThread`, `CreateFolder`, `RenameFolder`, `DeleteFolder`, `MoveFolder`, `PlanThreadDeletion` → `DeletionPlan{token}`, `DeleteThread{token}`, `CreateTab{agent, task_id, subtask_id, start_task_agent}`, `ListAgents`, `GetAgentUsage`, `CloseTab`, `RenameTab`, `SplitPane`, `ClosePane`, `FocusPane`, `ClearAttention`, `MoveDivider`, `ResizePaneEdge`, `Equalize`. The daemon sends `WorkspaceChanged` events to subscribers.
- **Terminal:**
  - `AttachPane{pane, cols, rows}` returns a full `ScreenUpdate`. After that the daemon streams `ScreenUpdate{seq}` events, and the client must send the `AckScreen{seq}` notification before the next one is sent.
  - `DetachPane`, `SetPaneSize`, the `Input{key|text|paste|mouse|wheel}` and `ScrollPane` notifications, and `ReadScreenText` (plain text, for the CLI and tests).
  - Events: `PaneExited`, `PaneBell`.
- **Task boards:** `CreateBoard`, `RenameBoard`, `DeleteBoard`, `CreateColumn`, `UpdateColumn`, `MoveColumn`, `DeleteColumn`, `CreateTask`, `UpdateTask`, `MoveTask`, `NestTask`, `DeleteTask`, `CreateSubtask`, `UpdateSubtask`, `DeleteSubtask`, `AddAttachment`, `GetAttachment`, `DeleteAttachment`. Boards arrive with the workspace (`Workspace.boards`), so subscribers see changes through `WorkspaceChanged`.
- **Settings:** `GetSettings`, `UpdateSettings` (global and per repo).
- **Remote:** `TailnetStatus`, `SetTailnetServe`.

## Screen encoding

`ScreenUpdate` carries:

- only the lines that changed, each as styled spans `(text, Style{fg, bg, flags})`. Trailing default blanks are trimmed. With `STYLE_FLAG_WIDE`, every character in the span is two columns wide.
- colours as `Color{indexed | rgb}`; unset means the theme default. The client's theme resolves the palette.
- cursor position and shape
- terminal modes that matter to the client (e.g. mouse reporting)
- the title set by the application, plus scrollback position (`display_offset`, `history_size`)

A full snapshot uses the same shape with every row included.
