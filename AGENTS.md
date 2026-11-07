# AGENTS.md

Guide for AI agents and humans contributing to **Urushi**. Read this file fully before you change anything. It is short on purpose. Details live in `docs/`.

## What Urushi is

Urushi is a **cross-platform graphical workspace for AI agents**. It runs on macOS, Linux, Windows, iOS and Android. It is a windowed GUI that *contains* terminals; it is **not** a terminal/TUI app.

- The sidebar lists **Projects → Threads**. Every thread is backed by its own **git worktree**.
- A thread has **tabs**. Each tab is a tree of **split panes**, and each pane is a terminal.
- Terminal sessions are **recoverable**. A headless in-house multiplexer inside the `urushid` daemon keeps processes and screen state alive across window close, app crash or client disconnect.
- Mobile apps are remote-only clients. They reach a desktop or server daemon over **Tailscale**.

All documentation is indexed in [`docs/README.md`](docs/README.md): architecture, decisions (ADRs), features, protocol and dependencies.

## Documentation is part of every change

We build up the documentation of our architecture, features and decisions **as we go**, in the same change as the code. This is not optional.

1. **Before starting a task:** use the [`find-docs`](.claude/skills/find-docs/SKILL.md) skill. Read the relevant feature doc, the ADRs and the module docs. Accepted ADRs are binding.
2. **While working:** if you make a real choice between alternatives, record it as an ADR ([`write-adr`](.claude/skills/write-adr/SKILL.md)).
3. **Before finishing:** use the [`update-docs`](.claude/skills/update-docs/SKILL.md) skill. Update or create the feature doc, update `docs/architecture.md` if boundaries moved, and keep `docs/README.md` indexed. Run `make check-docs`.

Docs describe what is true *now*. Delete stale text rather than appending corrections.

## Crate map

Crates marked ⏳ are planned but don't exist yet. Create them where listed; don't invent new top-level crates without an ADR.

| Crate | Role |
|---|---|
| `crates/urushi-core` | Pure domain model: ids, projects, threads, tabs, layout tree, settings, branch-ownership rules. **No IO, no async, compiles on every target.** |
| `crates/urushi-proto` | Protobuf types (prost + protox) and the length-prefixed frame codec. |
| `crates/urushi-mux` | `Pty` / `Emulator` traits + adapters, the per-pane Session actor, screen diffs, key encoding. |
| ⏳ `crates/urushi-daemon` | `urushid`: server, request router, SQLite store, git worktree lifecycle, Tailscale. |
| ⏳ `crates/urushi-client` | Protocol client, state mirror, transports (Unix socket / named pipe / TCP), daemon launcher. |
| ⏳ `crates/urushi-ui` | In-house widget toolkit (Elm-style). Produces a renderer-agnostic `Scene`. |
| ⏳ `crates/urushi-render` | wgpu backend: quads, glyph atlas (cosmic-text), terminal-grid pipeline. |
| ⏳ `crates/urushi-app` | Screens: sidebar, tabs, splits, dialogs, settings. Platform-neutral. |
| ⏳ `crates/urushi-cli` | `urushi` CLI for scripting and headless verification. |
| ⏳ `apps/{desktop,ios,android}` | Thin platform entry points only. |

## Invariants (never break these)

1. **The daemon is the single source of truth.** Clients render state and send intents. Workspace, layout, terminal emulation, key encoding and git all live in the daemon. Never duplicate that logic in a client.
2. **`urushi-core` has no IO.** No filesystem, network, processes, clock reads, tokio, or platform `cfg`. Inject time and randomness as values (e.g. pass ids in rather than generating them deep inside).
3. **Platform code is isolated.** `#[cfg(target_os = …)]` may appear only inside a `platform/` module, behind a platform-neutral API.
4. **Only `urushi-render` touches wgpu.** UI code builds a `Scene`, and the renderer draws it.
5. **The protocol is backward-compatible.** Changes are additive, and `buf breaking` must pass. Use the `change-protocol` skill.
6. **Licensing: permissive only.** Every external dependency must pass `cargo deny check` and be listed in [`docs/dependencies.md`](docs/dependencies.md). Copyleft (GPL/LGPL/AGPL/MPL/EPL/SSPL/BUSL) is never allowed. Use the `add-dependency` skill.
7. **Destructive git is gated.** Worktree or branch deletion happens only through `PlanThreadDeletion` → user confirmation → `DeleteThread(token)`. A thread may only delete branches it **owns**. That never includes the base branch, the default branch, branches that existed before the thread, or branches checked out in another worktree.
8. **Every thread has a worktree.** Creating a thread is all-or-nothing: if the worktree can't be created, the thread doesn't exist.

## Code quality rules

- **Errors:** `thiserror` enums in libraries, `anyhow` only in binaries. No `unwrap`/`expect`/`panic!` outside tests or one-time startup code. Clippy warns on them.
- **Unsafe:** denied workspace-wide. To use it, put `#[allow(unsafe_code)]` on the smallest module possible, with a `// SAFETY:` comment on every block.
- **Lints:** `[workspace.lints]` in `Cargo.toml` (clippy pedantic). CI runs with `-D warnings`. Fix lints rather than allow them. If an `allow` is truly needed, scope it tightly and give a reason.
- **Docs:** every public item gets a doc comment explaining *why* and *what*, not *how*. Module docs state the module's responsibility.
- **Size:** aim for ≤ ~400 lines per file and one responsibility per module. Split before it grows.
- **Types over comments:** use newtypes for ids and validated values (e.g. `BranchPrefix`), and constructors that enforce invariants. Make invalid states unrepresentable.
- **Async:** actor + mailbox (tokio tasks owning their state). Never hold a lock across `.await`. Bound every channel and buffer.
- **Dependencies:** declare versions once in the root `[workspace.dependencies]`. Crates use `dep = { workspace = true }`.
- **Naming:** say *pane* (one terminal), *tab* (layout of panes), *thread* (worktree + tabs), *project* (git repo). Don't use "window" or "session" in the domain model.

## Commands

```sh
make fmt              # format everything
make lint             # fmt check + clippy -D warnings
make test             # all tests
make check-licenses   # cargo deny check
make check-targets    # cargo check for iOS + Android targets
make check-docs       # docs indexed, links resolve, every crate documented
make ci               # everything CI runs; must be green before you finish
```

Toolchain: Rust is pinned in `rust-toolchain.toml` (needs rustup; make sure `~/.cargo/bin` comes before Homebrew in `PATH`). Extra tools: `cargo-deny`, `buf`.

## Testing expectations

- **Pure logic** (`urushi-core`): unit tests next to the code, plus **property tests** in `tests/` for structural invariants (e.g. layout operations).
- **Mux:** integration tests against a real PTY (`/bin/sh -c 'printf …'`). They must pass on macOS, Linux and Windows.
- **Git:** temporary repos plus a local bare repo standing in for the remote. Never touch the developer's real repos.
- **UI:** test `update()` directly. Snapshot `Scene` output with `insta` (no GPU needed).
- **Protocol:** round-trip encode/decode tests for every message.
- **End-to-end without the GUI:** drive `urushid` with the `urushi` CLI (`send-keys`, `dump-screen`). Prefer this over manual testing when verifying behaviour.

Bug fixes start with a failing test.

## Definition of done

1. `make ci` is green.
2. Tests cover the new behaviour, including failure paths.
3. Docs are updated in the same change (`update-docs` skill): feature doc, architecture, ADR for any real decision, and the index in `docs/README.md`.
4. Any new dependency is recorded in `docs/dependencies.md` with a reason.

## Skills

Skills live in `.claude/skills/`. Agents that don't load skills automatically should open the `SKILL.md` directly.

| Skill | Use when |
|---|---|
| [`find-docs`](.claude/skills/find-docs/SKILL.md) | **Start of every task**: locate the relevant architecture, ADRs, feature docs and module docs. |
| [`update-docs`](.claude/skills/update-docs/SKILL.md) | **End of every task**: create or update the docs the change affects. |
| [`add-dependency`](.claude/skills/add-dependency/SKILL.md) | Adding or upgrading any crate, tool or asset from outside the repo. |
| [`change-protocol`](.claude/skills/change-protocol/SKILL.md) | Touching anything in `proto/` or the frame codec. |
| [`write-adr`](.claude/skills/write-adr/SKILL.md) | Making or reversing an architectural decision. |
