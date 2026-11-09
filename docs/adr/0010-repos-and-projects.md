# 0010. Rename "projects" to repos, and add projects as groups of threads

- **Status:** Accepted
- **Date:** 2026-11-07

## Context
Until now a *project* was a git repository. Users want to organize threads across
repositories: a "Billing launch" project may hold threads from the api and the web repo.
So the word *project* must move to the new concept, and repositories become *repos*.
Two things already depend on the old names:
- **The wire protocol.** `buf breaking` uses `WIRE_JSON` (ADR 0003), so v1 field names
  such as `projects`, `project_id` and the `add_project` oneof are frozen.
- **The store** (ADR 0008). `workspace.json` has a top-level `projects` key holding repos,
  and each thread has `"project": <repo id>`.

## Decision
- **Domain, code, UI, CLI and docs** use the new names: `urushi_core::Repo` / `RepoId`
  (git repositories) and `urushi_core::Project` / `ProjectId` (user-made groups of
  threads). A thread has `repo` (always) and `project: Option<ProjectId>`. CLI: `urushi
  repo …` and `urushi project …`.
- **The v1 wire keeps its names for repos**: `Project`, `projects`, `project_id`,
  `AddProject`, … all mean repos. Projects travel as **`Folder`** (`Workspace.folders`,
  `Thread.folder_id`, `CreateFolder`/`RenameFolder`/`DeleteFolder`), added in protocol 1.2.
  `urushi-proto::convert` is the one place that maps the names
  (`repo_to_proto` ↔ `v1::Project`, `project_to_proto` ↔ `v1::Folder`).
- **The store** writes `repos` and `folders`, reads the old `projects` key as repos
  (`#[serde(alias)]`), and a thread's `repo` also reads the old `project` key. The
  organizer field is stored as `folder` because `project` is taken by the alias. The
  format version stays 1: old files load unchanged.
- Organization (projects, pins, statuses) is daemon state like everything else
  (ADR 0002), so every client and the CLI see the same sidebar. Collapsed sections and
  the open sidebar tab are per-client view state.

## Alternatives considered
- **Keep "project" for repos and call the groups something else** ("folders",
  "workspaces"): contradicts what the user asked for, and the UI would teach a name the
  code doesn't use.
- **Rename the wire fields too**: breaks `WIRE_JSON` compatibility, and with it older
  clients (e.g. a phone that hasn't updated). A `urushi.v2` package for a rename is far
  more work than one mapping module.
- **Switch `buf breaking` to `WIRE` rules** so renames pass: silently gives up the JSON
  guarantee ADR 0003 chose.
- **Bump the store format and migrate**: unnecessary; aliases read old files and the
  new keys don't collide.

## Consequences
- Wire and code names differ for this concept. The note at the top of
  `proto/urushi/v1/workspace.proto`, `docs/protocol.md` and `convert.rs` say so; anyone
  touching the protocol must keep the mapping straight.
- A downgraded daemon would not find repos under the new `repos` key. Downgrades are not
  supported.
