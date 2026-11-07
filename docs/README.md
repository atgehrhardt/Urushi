# Urushi documentation

This is the map of all project documentation. **Keep it current.** Every doc file must be listed here (`make check-docs` enforces this).

| Kind | Where | Answers |
|---|---|---|
| Overview | [`architecture.md`](architecture.md) | How the system fits together: components, data flow, boundaries. |
| Decisions | [`adr/`](adr/) | *Why* we chose X over Y. Immutable once accepted, superseded rather than edited. |
| Features | [`features/`](features/) | What each user-visible capability does, how it flows through the crates, its invariants, tests and limitations. |
| Protocol | [`protocol.md`](protocol.md) | Client/daemon wire format, message groups, versioning. |
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

## Features
- [_template](features/_template.md)
- [Split-pane layouts](features/split-pane-layouts.md)
- [Thread worktrees and safe deletion](features/thread-worktrees.md)
