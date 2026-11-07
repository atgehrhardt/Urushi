# Dependencies & license policy

**Rule:** build in-house by default. Outside code is allowed only when it is
(a) substantial enough that owning it would be expensive, and
(b) under a **permissive license that allows free commercial use**.

`cargo deny check` enforces the allowlist in `deny.toml`. Each entry below records *why* we accepted the dependency, so we can revisit it later.

## Allowed licenses

MIT, MIT-0, Apache-2.0 (incl. LLVM exception), BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, CC0-1.0, Unicode-3.0, Unlicense.
OFL-1.1 is allowed **for bundled font files only**.

**Never allowed:** GPL, LGPL, AGPL, MPL, EPL, CDDL, SSPL, BUSL, "Commons Clause", or "free for non-commercial use" licenses.
An exception requires an ADR and a matching `exceptions` entry in `deny.toml`.

External programs we *run but do not link or ship* (`git`, `tailscale`) are not dependencies in this sense. They are documented in `docs/architecture.md`.

## Approved Rust crates

| Crate | License | Used by | Why not in-house |
|---|---|---|---|
| `serde` | MIT/Apache-2.0 | core | Standard way to serialize data; writing our own derive macros isn't worth it. |
| `thiserror` | MIT/Apache-2.0 | libraries | Derive macro for error types; zero runtime cost. |
| `uuid` | MIT/Apache-2.0 | core, daemon | Standard UUID type. The daemon generates time-ordered v7 ids. |
| `proptest` *(dev)* | MIT/Apache-2.0 | tests | Property testing with automatic shrinking of failing cases. |
| `tokio` | MIT | daemon, client, mux | Async runtime. Writing our own isn't sensible. |
| `tracing`, `tracing-subscriber` | MIT | daemon, mux | Structured logging. |
| `anyhow` | MIT/Apache-2.0 | binaries only | Error context in `main`. |
| `clap` | MIT/Apache-2.0 | `urushid`, `urushi` | Flag and subcommand parsing with help output. |
| `prost`, `prost-build` | Apache-2.0 | proto | Protobuf code generation and encoding. |
| `protox` | MIT/Apache-2.0 | proto (build) | Pure-Rust `.proto` compiler, so builds don't need `protoc`. |
| `portable-pty` | MIT | mux | PTYs on Unix and Windows ConPTY. Behind the `Pty` trait. |
| `alacritty_terminal` | Apache-2.0 | mux | VT emulation. Behind the `Emulator` trait (ADR 0001). |
| `serde_json` | MIT/Apache-2.0 | daemon | Store document (ADR 0008) and the Tailscale CLI's JSON. |
| `tempfile` *(dev)* | MIT/Apache-2.0 | tests | Temp dirs for git and daemon tests. |

### Planned (approve when first added)

| Crate | License | Purpose |
|---|---|---|
| `winit` | Apache-2.0 | Windowing and input on all five platforms |
| `wgpu` | MIT/Apache-2.0 | GPU rendering (Metal/Vulkan/DX12/GLES) |
| `cosmic-text` | MIT/Apache-2.0 | Text shaping and rasterization (includes swash and fontdb) |
| `accesskit`, `accesskit_winit` | MIT/Apache-2.0 | Accessibility tree |
| `arboard` | MIT/Apache-2.0 | Clipboard |
| `rfd` | MIT | Native file pickers (desktop) |
| `android-activity` | MIT/Apache-2.0 | Android entry point |
| `windows-sys` | MIT/Apache-2.0 | Win32 APIs (named pipes, ACLs) |
| `insta` *(dev)* | Apache-2.0 | Snapshot tests |

## Tools

| Tool | License | Purpose |
|---|---|---|
| `cargo-deny` | MIT/Apache-2.0 | License, advisory and source audit |
| `buf` | Apache-2.0 | Protobuf lint and breaking-change checks |
| `XcodeGen` | MIT | iOS project generation (no committed `.xcodeproj`) |
| `cargo-ndk` | MIT/Apache-2.0 | Android builds |

## Assets

| Asset | License | Purpose |
|---|---|---|
| JetBrains Mono *(planned)* | OFL-1.1 | Default terminal font |
| Inter *(planned)* | OFL-1.1 | Default UI font |

## Written in-house instead of depending on a crate

| Need | Where | Why |
|---|---|---|
| Slugs / branch-name validation | `urushi-core::naming` | ~100 lines, and has to match our git ref rules exactly. |
| Per-OS data directories and socket paths | `urushi-proto::platform` | A few env-var lookups don't need a dependency. |
| Workspace persistence | `urushi-daemon::store` | One atomically-written JSON file; SQLite would add a C dependency (ADR 0008). |
| Git integration | `urushi-daemon::git` | We run the user's `git` binary; libgit2 is GPL with a linking exception (ADR 0004). |
| Tailscale integration | `urushi-daemon::tailscale` | We read the installed CLI's JSON output; nothing is linked (ADR 0005). |
| Key encoding (xterm) | `urushi-mux::keys` | A small table that depends on terminal modes. |
