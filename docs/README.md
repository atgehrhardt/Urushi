# Urushi documentation

This is the map of all project documentation. **Keep it current.** Every doc file must be listed here (`make check-docs` enforces this).

| Kind | Where | Answers |
|---|---|---|
| Overview | [`architecture.md`](architecture.md) | How the system fits together: components, data flow, boundaries. |
| Decisions | [`adr/`](adr/) | *Why* we chose X over Y. Immutable once accepted, superseded rather than edited. |
| Features | [`features/`](features/) | What each user-visible capability does, how it flows through the crates, its invariants, tests and limitations. |
| Protocol | [`protocol.md`](protocol.md) | Client/daemon wire format, message groups, versioning. |
| Platforms | [`platforms.md`](platforms.md) | What runs where, how to build each platform, what has been verified. |
| UI toolkit | [`ui-toolkit.md`](ui-toolkit.md) | How the GUI is built: Elm loop, nodes, layout, scene, renderer, accessibility. |
| Dependencies | [`dependencies.md`](dependencies.md) | License policy, the approved crates and tools, and things we wrote ourselves instead. |
| Code-level | rustdoc (`cargo doc --open`) | Module and API contracts. Every crate's `lib.rs` states its responsibility. |

## Architecture decision records
- [0000 – template](adr/0000-template.md)
- [0001 – In-house, headless terminal multiplexer](adr/0001-in-house-multiplexer.md)
- [0002 – Daemon is the single source of truth](adr/0002-daemon-source-of-truth.md)
- [0003 – Versioned protobuf over length-prefixed frames](adr/0003-protobuf-protocol.md)
- [0004 – One worktree per thread via the git CLI](adr/0004-git-cli-worktrees.md)
- [0005 – Remote access via the installed Tailscale](adr/0005-tailscale-remote.md)
- [0006 – All-Rust graphical app (winit + wgpu + in-house toolkit)](adr/0006-all-rust-gpu-gui.md)
- [0007 – Platform code isolated in `platform/` modules](adr/0007-platform-isolation.md)
- [0008 – Workspace persisted as one atomically-written JSON document](adr/0008-json-document-store.md)
- [0009 – Start agent CLIs by typing their command into the pane's shell](adr/0009-agents-typed-into-a-shell.md)
- [0010 – Rename "projects" to repos, and add projects as groups of threads](adr/0010-repos-and-projects.md)
- [0011 – Read agent plan usage in the daemon, from each agent's own sign-in and logs](adr/0011-agent-usage-from-the-agents-own-sources.md)
- [0012 – Keep subtasks as a task's checklist, and let the daemon link threads to tasks](adr/0012-subtasks-as-checklists-and-thread-links.md)
- [0013 – Bring panes back after a daemon restart from per-pane restore points](adr/0013-restore-points-for-panes.md)
- [0014 – Start a task's agent with the task as its prompt, passed through the pane's environment](adr/0014-task-prompt-via-pane-environment.md)
- [0015 – Make subtasks full tasks nested inside their parent](adr/0015-subtasks-are-nested-tasks.md)
- [0016 – Keep attachments in the daemon, send their bytes over the protocol, and decode images there](adr/0016-daemon-stores-attachments.md)
- [0017 – Learn when an agent finished or waits from what it writes to its pane's terminal](adr/0017-agent-attention-through-the-pane-terminal.md)
- [0018 – Start a task in an existing thread through `CreateTab`, linking it there](adr/0018-start-tasks-in-existing-threads.md)
- [0019 – Draw frosted glass in our own renderer, with a backdrop-blur pass](adr/0019-in-app-frosted-glass.md)
- [0020 – Follow Jira boards from the daemon: Jira is the source of truth, edits write through, polling keeps them current](adr/0020-jira-boards-as-daemon-mirrors.md)
- [0021 – Make HTTPS requests through `curl --config -`, and keep the Jira sign-in in a private file](adr/0021-https-through-curl-and-a-private-credentials-file.md)
- [0022 – Read a Jira issue in full when its editor opens, and edit any field from Jira's own metadata](adr/0022-jira-issues-in-full-on-demand.md)
- [0023 – Read a Jira board someone looks at every 3 s, look before reading, and show moves at once](adr/0023-near-real-time-jira-sync.md)
- [0024 – Start a task's agent with a reference to the task and the CLI to read it, not a copy](adr/0024-task-prompts-by-reference.md)
- [0025 – Write a new subtask in an editor of its own, kept as a draft until Save](adr/0025-new-subtasks-in-their-own-editor.md)
- [0026 – Name a Jira issue's thread branch after its key, and let Jira's integration link the code](adr/0026-issue-keys-in-branch-names.md)
- [0027 – Show a Jira board's epics as cards, and create them like any issue](adr/0027-epics-are-cards-on-jira-boards.md)
- [0028 – Start a new thread from its base's state on the remote, without touching the user's checkout](adr/0028-start-threads-from-the-bases-remote-state.md)
- [0029 – Give projects a parent branch, and a parent-branch thread that works on it](adr/0029-project-parent-branch-threads.md)
- [0030 – Draft a task's message by pasting it into the agent once it says it is ready](adr/0030-draft-task-messages-once-the-agent-is-ready.md)
- [0031 – Keep one thread-less quick terminal per daemon, beside the tabs](adr/0031-thread-less-quick-terminal.md)
- [0032 – Release macOS builds from a version tag, for Apple Silicon, as an ad-hoc signed zip](adr/0032-tagged-macos-releases.md)
- [0033 – Name the CLI by its absolute path in task prompts](adr/0033-task-prompts-name-the-cli-by-path.md)
- [0034 – Delete Jira issues from Urushi, with their sub-tasks, after confirming](adr/0034-delete-jira-issues-after-confirming.md)
- [0035 – Treat Claude as finished only when no background agent of its own still runs](adr/0035-claude-is-done-when-no-background-agent-runs.md)
- [0036 – Show a thread's pull request: read a Jira issue's from Jira's development info, and let a local task link one](adr/0036-pull-requests-from-jira-dev-info-and-local-links.md)
- [0037 – Write quick filters to Jira through its board-settings endpoint, and keep them here when Jira refuses](adr/0037-quick-filters-written-to-jira-or-kept-here.md)
- [0038 – Reorder threads by moving them in the one stored list, and let a drop join the target's group](adr/0038-reorder-threads-in-the-stored-list.md)
- [0039 – Learn that an agent is running from its prompt and tool hooks, and end it on Stop, prompts, session end or an interrupt key](adr/0039-running-agents-from-their-hooks.md)
- [0040 – Release the Android APK from the same tag, signed with a release key kept in repository secrets; no iOS release yet](adr/0040-release-android-apks-signed-with-a-repository-secret-key.md)
- [0041 – Allow LAN access only from paired devices, through Noise, on private addresses](adr/0041-lan-access-through-paired-noise-channels.md)
- [0042 – Open Android's soft keyboard with JNI calls from the Rust shell](adr/0042-android-soft-keyboard-through-jni-from-the-shell.md)

## Features
- [_template](features/_template.md)
- [Split-pane layouts](features/split-pane-layouts.md)
- [Thread worktrees and safe deletion](features/thread-worktrees.md)
- [Recoverable terminal sessions](features/recoverable-sessions.md)
- [Remote access over Tailscale](features/remote-access.md)
- [LAN access (paired devices, no Tailscale)](features/lan-access.md)
- [The workspace GUI](features/gui-workspace.md)
- [Agent launcher](features/agent-launcher.md)
- [Organizing threads (projects, pins, statuses)](features/thread-organization.md)
- [A project's parent branch and its parent-branch thread](features/project-parent-branch.md)
- [Task boards](features/task-boards.md)
- [Jira boards](features/jira-boards.md)
- [Task agent and first-run setup](features/task-agent.md)
- [Agent usage in the status bar](features/agent-usage.md)
- [Agent attention and activity (finished / running / needs input)](features/agent-attention.md)
- [Quick terminal](features/quick-terminal.md)
- [CI and releases](features/ci-and-releases.md)
