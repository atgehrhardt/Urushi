# Feature: Jira boards

- **Status:** Shipped (Jira Cloud, API-token sign-in, polling sync, write-through edits)
- **Owners (crates):** `urushi-core` (`board/{jira,sync,kind}.rs`), `urushi-daemon` (`http.rs`, `exec.rs`, `jira/{api,cloud,parse,adf,credentials}.rs`, `service/{jira,jira_write}.rs`, `server/jira.rs`, the Jira branch in `service/boards.rs`), `urushi-proto` (`jira.proto`, `convert/boards/jira.rs`), `urushi-client` (`jira.rs`, `TaskRef::{kind, issue}`), `urushi-ui` (`TextField::secret`), `urushi-app` (`update/jira.rs`, `msg/jira.rs`, `view/jira.rs`, `view/board/jira.rs`, `view/tasks/jira.rs`, `view/menu/jira.rs`, `snapshot/jira.rs`), `urushi-cli` (`urushi jira`, `urushi board sync`)
- **Related ADRs:** [0020](../adr/0020-jira-boards-as-daemon-mirrors.md), [0021](../adr/0021-https-through-curl-and-a-private-credentials-file.md), [0002](../adr/0002-daemon-source-of-truth.md), [0015](../adr/0015-subtasks-are-nested-tasks.md)

## What it does
Any Jira Cloud board can be imported into the Tasks panel. It keeps Jira's columns and stays
in sync both ways: changes made in Jira show within about 15 s while the app is open, and
changes made here go to Jira first.

- **Sign in:** the board panel's **+** offers **New board…** and **Import from Jira…**.
  The first time, **Connect Jira** asks for the site (`acme`, `acme.atlassian.net`), the
  Atlassian email and an API token (a hidden field; **Open** goes to the token page). Jira
  checks them before the daemon keeps them. Settings shows who the daemon is signed in as,
  with **Disconnect**, or **Connect Jira…**.
- **Import:** a list of the site's boards (kanban or scrum, and their project), searched
  with Enter; imported ones are marked. Choosing one adds a tab with a small **J** badge
  (red when syncing fails).
- **On the board:** the header shows "Synced 2 min ago" (or why syncing failed), **Sync
  now**, **Open in Jira** and **Remove**, which removes the board here and changes nothing in
  Jira. **All epics ▾** filters by epic, next to the tag filter. Cards show a marker in the
  issue type's color (story green, bug red and round, task blue, epic purple), the issue
  key (not on phones), an epic chip, Jira labels as tags, subtask progress and the
  assignee's initials, at the same size as any card.
- **Changes go to Jira:** dragging a card (or the editor's column chips) takes a
  transition to a status the column shows, then ranks it among the column's cards; a move
  the workflow doesn't allow is refused with a message and the card stays. Title, notes
  and tags (labels; no spaces) are edited in Jira. Checking a subtask off transitions it to
  a done status, unchecking to a to-do one. **+** on a column creates an issue of the type
  picked in the editor (task, story or bug), with its subtasks, moved to that column, and
  only on **Save**: Enter in the title moves on to the notes, and subtasks added in the
  editor are created in Jira on Save too.
- **Where an issue is:** a subtask opened from its parent's editor is headed like a Jira
  issue: **‹** back to the parent (edits kept), the epic, `PARENT-1 /`, then its own kind
  and key. The parent's subtask rows show each subtask's kind and key.
- **Urushi's own:** a card's color and the thread started for it stay here and survive every
  sync; threads start from Jira cards like any other.
- **Left to Jira:** deleting issues, nesting them, changing their type, changing columns,
  renaming the board and attachments. The editor shows the issue's key (**Open in Jira**),
  type, status, assignee, priority and epic read-only, and has no Delete; the card menu
  offers **Open in Jira** instead. A description too long to show whole is read-only.
- **Task kinds on every board:** any task can be a task, story, bug or epic (the editor's
  **Type**), shown by the same marker.

## How it works
- **Core** (`urushi_core::board`): `Board.{source: Option<BoardSource::Jira(JiraBoard)>,
  epics}`, `Column.statuses`, `Task.{kind: Option<TaskKind>, issue: Option<Issue>}`, all
  serde-defaulted. `Board::new_jira` and `set_jira_columns` build columns from Jira's
  configuration (same name keeps id and color). `Board::merge_issues(snapshot, full,
  new_id)` (`sync.rs`) re-places every task from its issue (column by status, rank order,
  subtasks under their parent by `Issue.parent`, epics into `Board.epics`), keeps ids,
  links and colors, removes issues missing from a full snapshot, and bounds the board
  (`MAX_SYNCED_ISSUES`, `MAX_SUBTASKS`, `MAX_NOTES`). `local_only` refuses what Jira owns
  with `BoardError::FollowsJira` (`FAILED_PRECONDITION` on the wire). `TaskKind::from_jira`
  maps a type by its hierarchy level and name.
- **Daemon, reading** (`jira/cloud.rs` over `http::CurlHttp`, ADR 0021): boards and their
  configuration from `/rest/agile/1.0/board`, issues from `/rest/api/3/search/jql` with
  `filter = <board filter> AND (<sub-query>) [AND sprint in openSprints()] AND
  (statusCategory != Done OR updated >= -14d) [AND updated >= -Nm] ORDER BY Rank`
  (paged by `nextPageToken`, with guards against tokens that repeat). `jira/parse.rs` reads
  answers leniently; `jira/adf.rs` turns ADF into plain notes and back.
- **Daemon, syncing** (`service/jira.rs`): `keep_jira_synced` ticks every 5 s and reads the
  boards that are due (`ACTIVE_INTERVAL` 15 s while a client subscribes, `IDLE_INTERVAL`
  2 min otherwise; a full read every `FULL_INTERVAL` 5 min, on import and on `SyncBoard`).
  Reads happen off the state lock; the merge runs in `mutate_if_changed`, which persists
  and wakes clients only if the board changed. Failures are written to
  `JiraBoard.sync.error` and back off (`backoff`: `Retry-After` within 1 min–1 h for a
  429). A per-board async lock keeps one sync or write at a time.
- **Daemon, writing** (`service/jira_write.rs`): `update_task`, `move_task`,
  `create_task`, `create_subtask` and `delete_task` in `service/boards.rs` branch to it for
  tasks on Jira boards: Jira first (`edit` with label add/remove operations, `transition`,
  `create` with the project's issue type for the kind, `rank`), then `issue` read back and
  merged. Only the color changes locally.
- **Sign-in** (`jira/credentials.rs`): `site_address` normalizes the site (https only);
  `CredentialStore` keeps it in `<home>/jira-credentials.json` (0600).
- **Wire (1.12):** `Board.{jira, epics}`, `BoardColumn.jira_statuses`, `Task/Subtask.{kind,
  issue}`, `CreateTask.kind`, `UpdateTask.kind`, and `ConnectJira` → `JiraAccount`,
  `GetJiraAccount`, `DisconnectJira`, `ListJiraBoards` → `JiraBoardList`,
  `ImportJiraBoard` → `Board`, `SyncBoard`; `ERROR_CODE_UNAVAILABLE` when Jira can't be
  reached. The token travels only in `ConnectJira`.
- **App:** `AppState.jira` (asked with `GetJiraAccount` on connect and when Settings
  opens), `Dialog::{ConnectJira, ImportJira}` (their errors show in the dialog, not the
  banner; `update/jira.rs::jira_failed`), `BoardPanel.epic_filter` (`board::passes`),
  `TaskLabels.kind`, `Effect::OpenUrl` (`platform::open_url`, https only). Gated on
  `daemon_supports_jira()` (`JIRA_SINCE_MINOR = 12`).
- **CLI:** `urushi jira connect --site --email` (token from `URUSHI_JIRA_TOKEN` or stdin),
  `jira status|disconnect|boards [query]|import <id>`, `urushi board sync <board>`,
  `board add|edit --kind`; `board ls` shows keys, kinds, epics, assignees and sync state.

## Invariants
- The API token never leaves the daemon: not in settings, the workspace, its document, logs
  or `Debug` output; its file is 0600
  (`tests/jira_boards.rs::the_sign_in_survives_restarts_and_never_leaves_the_daemon`,
  `jira/credentials.rs` tests, `http/tests.rs::debug_output_leaves_out_credentials`).
- Requests reach curl only through stdin (`http/tests.rs::requests_reach_curl_only_through_its_stdin`).
- Merging never duplicates an issue, keeps a task's id and link while its issue stays, never
  makes an epic a card, stays within the bounds, and merging the same snapshot again
  changes nothing (`urushi-core/tests/jira_sync_props.rs`, `board/sync/tests.rs`).
- A change Jira refuses changes nothing here
  (`tests/jira_boards.rs::moves_and_new_cards_take_jiras_transitions`, `what_jira_owns_is_refused`).
- Jira-owned changes are refused on Jira boards whoever asks
  (`board/sync/tests.rs::changes_jira_owns_are_refused_and_change_nothing`).

## Tests
- Core: `board/sync/tests.rs`, `board/kind.rs`, `tests/jira_sync_props.rs`.
- Proto: `convert/boards/jira.rs` (round trips, wire messages, issue addresses).
- Daemon: `http/tests.rs` (config quoting, a fake and the real curl), `jira/cloud/tests.rs`
  (requests, status mapping, paging), `jira/parse/tests.rs`, `jira/adf.rs`,
  `jira/credentials.rs`, `service/jira.rs` (backoff), and `tests/jira_boards.rs` end to end
  against an in-memory Jira (sign-in, import, remote changes, transitions, refused moves,
  edits, subtasks, refusals, restart, sign-out).
- UI: `textfield/secret.rs`. App: `update/tests/jira.rs` (including a new issue created
  only on Save); `snapshot/tests.rs` (`jira-board`, `jira-task`, `jira-subtask` (the top
  bar), `jira-connect`, `jira-import` scenarios, card sizes, no
  subtask delete on Jira boards, the Type picker).

## Limitations and follow-ups
- Jira Cloud with an API token only: no Data Center/Server (personal access tokens, API
  v2) and no OAuth sign-in.
- Polling, not webhooks: changes take up to 15 s (2 min with no app open) to arrive.
- Moves wait for Jira (about a second) before the card settles.
- Jira attachments, comments, sprints/backlog views and changing an issue's assignee,
  priority or type aren't shown or editable here.
- Editing notes replaces the description with plain paragraphs (formatting is lost).
- Issues whose status no column shows, and done issues older than 14 days, aren't on the
  board; a board shows at most 500 tasks.
- The sign-in is a file, not the OS keychain (ADR 0021). Claude's usage request still makes
  its own curl call instead of going through `HttpClient`.
- Opening Jira pages isn't wired to the mobile apps yet.
