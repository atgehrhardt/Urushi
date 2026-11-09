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

## Features
- [_template](features/_template.md)
- [Split-pane layouts](features/split-pane-layouts.md)
- [Thread worktrees and safe deletion](features/thread-worktrees.md)
- [Recoverable terminal sessions](features/recoverable-sessions.md)
- [Remote access over Tailscale](features/remote-access.md)
- [The workspace GUI](features/gui-workspace.md)
- [Agent launcher](features/agent-launcher.md)
- [Organizing threads (projects, pins, statuses)](features/thread-organization.md)
- [Task boards](features/task-boards.md)
- [Task agent and first-run setup](features/task-agent.md)
- [Agent usage in the status bar](features/agent-usage.md)
- [Agent attention (finished / needs input)](features/agent-attention.md)
