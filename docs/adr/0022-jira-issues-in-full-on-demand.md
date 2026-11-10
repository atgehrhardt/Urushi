# 0022. Read a Jira issue in full when its editor opens, and edit any field from Jira's own metadata

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
Users want to do everything on a Jira board here that they can do in Jira's issue view:
change the assignee, priority, type, parent, sprint, story points, due date,
components, fix versions and their site's own fields, and comment, attach files, link
issues, watch and log work (UR-3). ADR 0020 kept a small `Issue` on each task: what
cards show, mostly as display names, and only the summary, description and labels were
editable.

Every workspace change resends the whole workspace to every client and rewrites one
JSON document (ADR 0008). Sites differ: fields are custom, required fields vary per
issue type, and which fields an account may edit depends on the issue's screen and
permissions.

## Decision
- **The board keeps only what cards and filters need**, with ids so changes can be made:
  type, assignee and reporter (account ids), priority, due date, estimate, sprint,
  components, fix versions and a link count (protocol 1.13).
- **The rest is read on demand.** `GetJiraIssue` makes the daemon read the issue's edit
  metadata, the values of those fields, its comments, watchers, work log, transitions,
  the site's link kinds and the board's sprints, and answer with `JiraIssueDetails`.
  Nothing of it is stored; the daemon remembers only the kinds of the fields of issues
  opened lately, to write edits.
- **The form comes from Jira's metadata.** Each editable field is described by a kind
  (text, rich text, number, date, one of, several of, person, people, labels, issue,
  sprint, other) with its allowed values, so a site's own fields can be edited too.
  New issues use the create metadata of their type, and the app asks for the required
  ones before Save.
- **One change request.** `ChangeJiraIssue{task_id, oneof change}` covers field edits,
  transitions, comments, attachments, links, watching and work. As in ADR 0020, it is
  made in Jira first under the board's lock, the issue is read back and merged, and the
  answer is the issue in full as it is afterwards.
- **Rich text is Markdown.** Descriptions and comments convert between ADF and a
  Markdown subset both ways, so editing keeps headings, lists, marks, links and code.

## Alternatives considered
- **Keep everything on the board.** Comments, work logs and metadata for 500 issues
  would be resent on every change and written to the document every few seconds.
- **A fixed list of fields.** Simpler to draw, but misses custom fields, which is most
  of what teams configure, and can't know what is required on create.
- **Optimistic local edits.** Kept out for the same reasons as in ADR 0020: refusals
  (workflow, validation, permissions) would have to be undone afterwards.
- **Plain text descriptions (as before).** Every edit flattened formatting written in
  Jira.

## Consequences
- Opening a Jira issue costs several requests (about a second); the editor shows the
  board's summary until the answer arrives.
- Fields Urushi has no kind for show as text and can't be changed here (e.g. cascading
  selects).
- Markdown can't say everything ADF can: panels, colors and mentions as links read as
  text and are written back as text when the description is edited.
- The daemon depends on more of Jira Cloud's REST API (edit and create metadata,
  comments, attachments, links, watchers, work logs, user and issue search).
