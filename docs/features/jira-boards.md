# Feature: Jira boards

- **Status:** Shipped (Jira Cloud, API-token sign-in, polling sync, write-through edits, issues in full, epics as cards, quick filters made here)
- **Owners (crates):** `urushi-core` (`board/{jira,sync,kind,quick,jql}.rs`), `urushi-daemon` (`http.rs`, `exec.rs`, `time.rs`, `jira/{api,cloud,cloud/details,cloud/quick_filters,parse,parse/details,details,adf,credentials}.rs`, `service/{jira,jira_write,jira_query,jira_details,jira_edit,quick_filters}.rs`, `server/{jira,jira_issue,quick_filters}.rs`, the Jira branch in `service/boards.rs`), `urushi-proto` (`jira.proto`, `jira_issue.proto`, `convert/boards/jira/`), `urushi-client` (`jira.rs`, `jira_issue.rs`, `quick_filters.rs`, `TaskRef::{kind, issue}`), `urushi-ui` (`TextField::secret`), `urushi-app` (`filter.rs`, `filter/{quick,jql,editor}.rs`, `issue.rs`, `update/{jira,filter,issue,quick_filter}.rs`, `msg/{jira,filter,issue}.rs`, `view/jira.rs`, `view/board/{jira,filter,quick_filter}.rs`, `view/tasks/{jira,header}.rs`, `view/tasks/issue/`, `view/menu/{jira,filter,quick_filter}.rs`, `snapshot/{jira,jira_issue,quick_filters}.rs`), `urushi-cli` (`urushi jira`, `urushi board sync`, `urushi board quick`)
- **Related ADRs:** [0033](../adr/0033-quick-filters-written-to-jira-or-kept-here.md), [0020](../adr/0020-jira-boards-as-daemon-mirrors.md), [0021](../adr/0021-https-through-curl-and-a-private-credentials-file.md), [0022](../adr/0022-jira-issues-in-full-on-demand.md), [0023](../adr/0023-near-real-time-jira-sync.md), [0026](../adr/0026-issue-keys-in-branch-names.md), [0027](../adr/0027-epics-are-cards-on-jira-boards.md), [0025](../adr/0025-new-subtasks-in-their-own-editor.md), [0002](../adr/0002-daemon-source-of-truth.md), [0015](../adr/0015-subtasks-are-nested-tasks.md)

## What it does
Any Jira Cloud board can be imported into the Tasks panel. It keeps Jira's columns and stays
in sync both ways: changes made in Jira show within about 3 s on the board on screen (15 s
on others while the app is open), and changes made here go to Jira first; a dragged card
moves at once and goes back if Jira refuses.

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
  Jira. Cards show a marker in the
  issue type's color (story green, bug red and round, task blue, epic purple), the issue
  key (not on phones), an epic chip, Jira labels as tags, subtask progress and the
  assignee's initials, at the same size as any card.
- **Epics are cards** (ADR 0027): an epic sits in the column that shows its status, like
  any issue. It can be dragged, opened in full and used to start a thread. Its stories
  stay in their own columns with its chip, and filtering on the epic shows the epic with
  its stories. Jira epics hold stories, not sub-tasks: an epic's editor says so in place
  of "Add a subtask…", and the daemon refuses sub-tasks under an epic.
- **Filtering, as in Jira:** under the tabs, a search field (title, key or notes; Escape
  clears it, then lets go of the keyboard), the people issues are assigned to as
  avatars, **Only my issues**, **Recently updated** (the last day) and the board's own
  quick filters as chips, then **Filters ▾**: the same switches and a page per field
  (assignee, type, priority, status, label, epic, sprint, component, fix version,
  reporter) listing the values the board's issues have; several can be chosen. Values
  in one field widen it, fields narrow each other, and a card shows when it or one of
  its subtasks passes. Quick filters are JQL, so Jira evaluates them (asked again
  whenever the board changes while one is on); everything else is filtered here.
  Columns count "shown/all". On a phone the bar is the search and **Filters ▾**.
  Boards of Urushi's own get the search, type and label.
- **Making quick filters** (ADR 0033, 1.21 daemons): **Save filter…** in the bar (or
  **Save as quick filter…** in **Filters ▾**) saves what is chosen. The dialog asks for a
  name and shows the JQL built from the selection (people, fields, "Only my issues",
  "Recently updated", the search, and quick filters that are on), which can be changed.
  **Save** checks the JQL with Jira, then makes the quick filter on the board in Jira, so
  it shows there too. The board then shows just that filter, on. Right-click any quick
  filter chip, Jira's own included, for **Edit…** (name and JQL) and **Delete…**, which
  asks first and deletes it in Jira. If Jira won't take a new one (the account isn't a
  board admin, or Jira's endpoint for it changed), Urushi keeps it on the board, Jira
  still evaluates its JQL, and the banner says why.
- **Everything a Jira issue has, in its editor** (ADR 0022): opening an issue reads it
  in full. On the right: its key with **Watch**/**Watching (n)** and **Open in Jira**,
  **Status** (the transitions it can take, made at once), then every field Jira lets the
  account edit, in Jira's order (type, assignee with **Assign to me**, reporter,
  priority, parent, due date, components, fix versions, sprint, story points and the
  site's own fields), each as its kind needs: a text or date field, a list of allowed
  values, chips for several, people and issues searched in Jira (Enter). Changed fields
  are marked • and go to Jira with **Save**. Under the notes, tabs: **Comments** (add in
  Markdown; edit or delete your own), **Files** (open, add, delete), **Links** (open the
  other issue here if it's on a board, else in Jira; add "this issue blocks / is blocked
  by …", remove) and **Work** (time tracking, the log, and **Log work** with "1h 30m").
  Those are made in Jira at once. Descriptions and comments keep their formatting as
  Markdown. The editor is wider and scrolls; until Jira answers, the board's summary
  shows.
- **New issues** ask Jira which fields their type has; required ones (marked *) must be
  filled in before Save, and go with the new issue.
- **Changes go to Jira:** dragging a card (or the editor's column chips) takes a
  transition to a status the column shows, then ranks it among the column's cards; a move
  the workflow doesn't allow is refused with a message and the card stays. Title, notes
  and tags (labels; no spaces) are edited in Jira. Checking a subtask off transitions it to
  a done status, unchecking to a to-do one. **+** on a column creates an issue of the type
  picked in the editor (task, story, bug, or epic from a 1.15 daemon), with its subtasks
  (none for an epic), moved to that column, and
  only on **Save**: Enter in the title moves on to the notes, and subtasks added in the
  editor are created in Jira on Save too, each a Sub-task issue with what its own editor
  set (description, labels, the Sub-task type's fields, required ones asked for first;
  ADR 0025).
- **Where an issue is:** a subtask opened from its parent's editor is headed like a Jira
  issue: **‹** back to the parent (edits kept), the epic, `PARENT-1 /`, then its own kind
  and key. The parent's subtask rows show each subtask's kind and key.
- **Urushi's own:** where the board's tab sits among the others (drag it, or **Move
  left/right**; see [task boards](task-boards.md)), a card's color and the thread
  started for it stay here and survive every sync; threads start from Jira cards like any other.
- **Linked from code** (ADR 0026): a thread started from an issue (card or subtask) gets
  a branch led by that issue's key, as Jira writes it: the new-thread form suggests
  `urushi/UR-3-invoice-pdfs` (`update/intake.rs::suggested_branch`), and a thread
  created without a branch gets `urushi/UR-3-invoice-pdfs-<short id>`
  (`BranchName::for_issue_thread`, `State::task_issue_key`). The agent is asked to put the
  key in commit messages and pull request titles too. Jira's own GitHub, GitLab or
  Bitbucket integration then lists the branch, its commits and its pull requests on the
  issue; Urushi itself doesn't talk to the code host. Any number of threads, branches
  and pull requests can name one issue.
- **Left to Jira:** deleting issues, moving them between projects, changing columns and
  renaming the board. The editor has no Delete; the card menu offers **Open in Jira**
  instead. A description too long to show whole is read-only, and fields Urushi has no
  kind for (e.g. cascading selects) show as text.
- **Task kinds on every board:** any task can be a task, story, bug or epic (the editor's
  **Type**), shown by the same marker.

## How it works
- **Core** (`urushi_core::board`): `Board.{source: Option<BoardSource::Jira(JiraBoard)>,
  epics}`, `Column.statuses`, `Task.{kind: Option<TaskKind>, issue: Option<Issue>}`, all
  serde-defaulted. `Board::new_jira` and `set_jira_columns` build columns from Jira's
  configuration (same name keeps id and color). `Board::merge_issues(snapshot, full,
  new_id)` (`sync.rs`) re-places every task from its issue (column by status, rank order,
  subtasks under their parent by `Issue.parent`; epics are cards too, and `Board.epics`
  names every epic that is a card or that a task names), keeps ids,
  links and colors, removes issues missing from a full snapshot, and bounds the board
  (`MAX_SYNCED_ISSUES`, `MAX_SUBTASKS`, `MAX_NOTES`). `local_only` refuses what Jira owns
  with `BoardError::FollowsJira` (`FAILED_PRECONDITION` on the wire). `TaskKind::from_jira`
  maps a type by its hierarchy level and name.
- **Daemon, reading** (`jira/cloud.rs` over `http::CurlHttp`, ADR 0021): boards, their
  configuration (with the estimate field) and quick filters from `/rest/agile/1.0/board`,
  the site's sprint field from `/rest/api/3/field`, issues (with reporter, due date,
  components, fix versions, links, and the board's rank, estimate and sprint fields:
  `IssueFields`) from `/rest/api/3/search/jql` with
  `filter = <board filter> AND (<sub-query>) [AND sprint in openSprints()] AND
  (statusCategory != Done OR updated >= -14d) [AND updated >= -Nm] ORDER BY Rank`
  (paged by `nextPageToken`, with guards against tokens that repeat). `issue_ids` runs
  the same query with `IssueQuery.extra_jql` (the chosen quick filters) for ids only
  (`service/jira_query.rs`, `QueryJiraFilter`). `jira/parse.rs` reads
  answers leniently; `jira/adf.rs` turns ADF into plain notes and back.
- **Daemon, syncing** (`service/jira_sync.rs`, ADR 0023): `keep_jira_synced` ticks every
  second and starts a sync for each board that is due and not already syncing, side by
  side: every `WATCHED_INTERVAL` (3 s) while a connection shows it (`WatchBoard`; a
  `BoardWatch` the connection drops when it asks for another or closes),
  `ACTIVE_INTERVAL` (15 s) while a client subscribes, `IDLE_INTERVAL` (2 min) otherwise,
  and at once after a write (`poke`). A sync first asks `JiraApi::changed` (ids and
  update times, `fields=updated`), compares with `BoardSync.seen`, and reads in full only
  the issues whose time is new (`id in (…)`); a full read every `FULL_INTERVAL` (5 min),
  on import and on `SyncBoard` reads the configuration and everything. Reads happen off
  the state lock; the merge runs in `mutate_if_changed`, which persists and wakes
  clients only if the board changed. Failures are written to `JiraBoard.sync.error` and
  back off (`backoff`: `Retry-After` within 1 min–1 h for a 429). A per-board async lock
  keeps one sync or write at a time.
- **Daemon, writing** (`service/jira_write.rs`, and `service/jira_create.rs` for new
  issues (epics included, never with sub-tasks) and sub-tasks with their details): `update_task`, `move_task`,
  `create_task`, `create_subtask` and `delete_task` in `service/boards.rs` branch to it for
  tasks on Jira boards: Jira first (`edit` with label add/remove operations, `transition`,
  `create` with the project's issue type for the kind, `rank`), then `issue` read back and
  merged. Only the color changes locally, and a move shows first
  (`Board::show_jira_move`, undone with `undo_jira_move` if Jira refuses).
- **Daemon, quick filters** (ADR 0033, `service/quick_filters.rs`): `create_quick_filter`
  on a Jira board needs JQL (`BoardError::WrongQuickRule` otherwise). It checks the JQL
  with a one-issue `issue_ids` search (Jira's `Rejected` becomes `Invalid`), then calls
  `JiraApi::create_quick_filter`, all under the board's lock. Writes go through
  `jira/cloud/quick_filters.rs` to the board-settings endpoint
  (`POST|PUT|DELETE /rest/greenhopper/1.0/quickfilters/{board}[/{id}]`, with `name`,
  `query` and `description`). Afterwards `JiraApi::quick_filters` (the agile API) is read
  back into `JiraBoard.quick_filters` and the cached `BoardSync.config`, so syncs keep
  it. On `NotFound`, `Forbidden` or `Rejected` the filter goes into `Board.quick_filters`
  as `QuickRule::Jql` and `QuickFilterSaved.kept_here` says why. Other errors are
  returned. Edits and deletes of Jira's own filters go to Jira the same way, with no
  fallback. `query_jira_filter` also takes the board's own JQL filters
  (`kept_quick_filter_ids`).
- **Daemon, one issue in full** (ADR 0022): `JiraApi::details` (`jira/cloud/details.rs`:
  edit metadata, the values of those fields plus attachments, links and time tracking,
  the board's open sprints, comments, watchers, work log, transitions, link kinds)
  parsed by `jira/parse/details.rs` into `jira::details` types; a field's
  `FieldKind` comes from its schema, and `FieldValue::to_json` writes an edit as Jira
  takes it. `service/jira_details.rs` reads it (and remembers the fields' kinds per
  issue in `JiraLink::kinds`), create fields, attachments (decoded for thumbnails), and
  user and issue searches; `service/jira_edit.rs` makes an `IssueChange` under the
  board's lock, merges the issue read back, and answers with the details afterwards.
  Attachments go up as a multipart form through a private temporary file
  (`http::Upload`) and come down following Jira's redirect, from the site only.
  `jira/adf/` converts ADF ⇄ Markdown both ways.
- **Sign-in** (`jira/credentials.rs`): `site_address` normalizes the site (https only);
  `CredentialStore` keeps it in `<home>/jira-credentials.json` (0600).
- **Wire (1.21):** `CreateQuickFilter` → `QuickFilterSaved`, `UpdateQuickFilter`,
  `DeleteQuickFilter` (`QuickFilterRef`: Jira's id or a kept one's), `Board.quick_filters`,
  `JiraQuickFilter.jql`, `QueryJiraFilter.kept_quick_filter_ids`
  (`daemon_supports_quick_filters()`, `QUICK_FILTERS_SINCE_MINOR = 21`).
- **Wire (1.15):** no new fields: epic issues arrive as cards, and `CreateTask.kind =
  EPIC` is accepted on Jira boards (the app checks `daemon_supports_epic_cards()`,
  `EPIC_CARDS_SINCE_MINOR = 15`, before offering it).
- **Wire (1.13):** `JiraIssue.{type_id, assignee_id, reporter, priority_id, due, points,
  sprint, components, fix_versions, links}`, `JiraBoardSource.quick_filters`,
  `JiraAccount.account_id` (learned on the next full sync for a sign-in made before),
  `QueryJiraFilter` → `JiraIssueIds`.
- **Wire (1.12):** `Board.{jira, epics}`, `BoardColumn.jira_statuses`, `Task/Subtask.{kind,
  issue}`, `CreateTask.kind`, `UpdateTask.kind`, and `ConnectJira` → `JiraAccount`,
  `GetJiraAccount`, `DisconnectJira`, `ListJiraBoards` → `JiraBoardList`,
  `ImportJiraBoard` → `Board`, `SyncBoard`; `ERROR_CODE_UNAVAILABLE` when Jira can't be
  reached. The token travels only in `ConnectJira`.
- **App:** `filter.rs` (`BoardFilter`, `Dim`, `board_values`, `board_dims`; the filter is
  the window's own), `update/filter.rs` (`FilterMsg`, the search as `Field::BoardSearch`
  outside any dialog, quick-filter queries with `BoardPanel.{quick_asking,
  quick_stale}`), `view/board/filter.rs` (the bar), `view/menu/filter.rs` (the menu, a
  `MenuPage::Filter(Dim)` page per field, scrolling past ten values). Quick filters:
  `filter/jql.rs` (`selection_jql`: each dimension one `x in (…)` clause, values quoted
  with `urushi_core::board::jql`), `filter/quick.rs` (`QuickRef`), `filter/editor.rs`
  (`QuickFilterEditor` in `Dialog::QuickFilter`), `update/quick_filter.rs` (open, submit,
  answers, `Dialog::DeleteQuickFilter`), `view/board/quick_filter.rs`, and a chip's
  `MenuTarget::QuickFilter` (`view/menu/quick_filter.rs`). Kept JQL filters on
  (`BoardFilter.saved`) join the `QueryJiraFilter` request.
  `AppState.jira` (asked with `GetJiraAccount` on connect and when Settings
  opens), `Dialog::{ConnectJira, ImportJira}` (their errors show in the dialog, not the
  banner; `update/jira.rs::jira_failed`),
  `TaskLabels.kind`, `Effect::OpenUrl` (`platform::open_url`, https only). Gated on
  `daemon_supports_jira()` (`JIRA_SINCE_MINOR = 12`).
- **CLI:** `urushi jira connect --site --email` (token from `URUSHI_JIRA_TOKEN` or stdin),
  `jira status|disconnect|boards [query]|import <id>`, `jira quick <board> [ids…]`
  (the quick filters, or the issues they let through), `board quick
  ls|add|edit|rm` (`add <board> <name> --jql …`; prints whether it went to Jira or was
  kept here), `urushi board sync <board>`,
  `board add|edit --kind`; `board ls` shows keys, kinds, epics, assignees and sync state.

## Invariants
- The API token never leaves the daemon: not in settings, the workspace, its document, logs
  or `Debug` output; its file is 0600
  (`tests/jira_boards.rs::the_sign_in_survives_restarts_and_never_leaves_the_daemon`,
  `jira/credentials.rs` tests, `http/tests.rs::debug_output_leaves_out_credentials`).
- Requests reach curl only through stdin (`http/tests.rs::requests_reach_curl_only_through_its_stdin`).
- Merging never duplicates an issue, keeps a task's id and link while its issue stays,
  makes every epic a card (never a subtask) named in `Board.epics`, stays within the
  bounds, and merging the same snapshot again changes nothing (`urushi-core/tests/jira_sync_props.rs`, `board/sync/tests.rs`).
- An epic never gets sub-tasks: refused before Jira is asked
  (`tests/jira_boards.rs::epics_are_cards_and_can_be_created`).
- A change Jira refuses changes nothing here
  (`tests/jira_boards.rs::moves_and_new_cards_take_jiras_transitions`, `what_jira_owns_is_refused`).
- A quick filter Jira refuses changes nothing in Jira; one with JQL Jira can't read is
  never written or kept; one Jira wouldn't take is kept and survives syncs; a Jira write
  survives the next full sync (`tests/quick_filters.rs`).
- Jira-owned changes are refused on Jira boards whoever asks
  (`board/sync/tests.rs::changes_jira_owns_are_refused_and_change_nothing`).

## Tests
- Core: `board/sync/tests.rs`, `board/kind.rs`, `tests/jira_sync_props.rs`,
  `naming.rs` (`issue_keys_are_jiras_shape`,
  `issue_branches_start_with_the_key_as_jira_writes_it`), `board/prompt.rs`.
- Daemon: `tests/jira_boards.rs::threads_for_issues_get_branches_named_after_them`;
  `tests/quick_filters.rs` (made, changed and deleted in Jira; JQL checked first; kept
  here when Jira refuses, evaluated by Jira, through syncs; errors with no fallback);
  `jira/cloud/quick_filters.rs` (the requests sent and refusals).
- Core: `board/quick/tests.rs`, `tests/quick_filter_props.rs`, `board/jql.rs`.
- App: `update/tests/jira.rs::a_thread_for_an_issue_suggests_a_branch_led_by_its_key`;
  `update/tests/quick_filters.rs` (saving JQL, Jira's refusal in the banner, errors in
  the dialog, a chip's Edit and Delete, older daemons), `filter/jql.rs` (every
  dimension, quoting), and the `quick-filter-save` and `quick-filter-menu` snapshots.
- Proto: `convert/boards/jira/tests.rs` (round trips, wire messages, issue addresses).
- Daemon: `http/tests.rs` (config quoting, uploads and redirects, a fake and the real
  curl), `jira/cloud/tests.rs` (requests, status mapping, paging, opening an issue,
  comments, files, links, watching, work), `jira/parse/tests.rs`,
  `jira/parse/details/tests.rs` (field kinds and values), `jira/details.rs` (writing
  values), `jira/adf/` (Markdown round trips), `jira/credentials.rs`,
  `service/jira.rs` (backoff), `service/jira_edit.rs`, and end to end against an
  in-memory Jira (`tests/fake_jira/`): `tests/jira_boards.rs` (sign-in, import, remote
  changes, transitions, refused moves, edits, subtasks, epics, refusals, restart,
  sign-out, quick filters) and `tests/jira_issues.rs` (any field, comments, files, links,
  watching, work, searches, required create fields).
- UI: `textfield/secret.rs`. App: `filter/tests.rs` (each dimension, how they combine,
  subtasks keeping their card), `issue.rs` (durations, drafts), `update/tests/jira.rs` (a new issue created only on
  Save, field drafts sent with Save, changes made at once, required create fields,
  filters, quick-filter queries, the search's keyboard, a new epic, an epic under its own
  filter), `view/tasks/jira.rs` (Epic offered only from 1.15 daemons); `snapshot/tests.rs` (`jira-board`, `jira-task`, `jira-subtask` (the top
  bar), `jira-filters`, `jira-filter-menu`, `jira-issue`, `jira-issue-picker`, `jira-new-epic`, `jira-connect`, `jira-import` scenarios, card sizes, no
  subtask delete on Jira boards, the Type picker).

## Limitations and follow-ups
- Linking code needs the Jira site's GitHub (GitLab, Bitbucket) integration installed;
  Urushi doesn't check for it. A thread started in an existing thread (ADR 0018) keeps
  that thread's branch, so it links only through commits and pull request titles.
- Jira Cloud with an API token only: no Data Center/Server (personal access tokens, API
  v2) and no OAuth sign-in.
- Polling, not webhooks: changes take up to 3 s on the board on screen, 15 s on others,
  and 2 min with no app open, to arrive.
- Field edits other than moves wait for Jira (about a second) before the card changes.
- Sprint and backlog views aren't here; an issue's sprint is a field.
- Markdown can't say everything ADF can: panels, colors and mentions as links read as
  text and are written back as text when edited. Jira attachments don't show as
  thumbnails on cards.
- Opening an issue takes several requests (about a second) before its fields show.
- Issues whose status no column shows, and done issues older than 14 days, aren't on the
  board; a board shows at most 500 tasks, epics included. A board whose filter leaves
  epics out can't show a new epic ("Jira made KEY, but this board doesn't show it").
- An epic's editor doesn't list its stories (the Epic filter shows them). A story is put
  in an epic through its **Parent** field, once created.
- The sign-in is a file, not the OS keychain (ADR 0021). Claude's usage request still makes
  its own curl call instead of going through `HttpClient`.
- Opening Jira pages isn't wired to the mobile apps yet.
- Quick filters are written through an endpoint Jira doesn't document (ADR 0033): only
  board admins can write them, and if Atlassian changes the endpoint, new filters are
  kept here instead (edits and deletes fail with Jira's message). A filter kept here
  isn't moved to Jira later. Phones have no right-click, so they can't edit or delete
  quick filters yet.
