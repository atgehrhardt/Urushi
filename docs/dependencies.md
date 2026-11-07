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
| `uuid` | MIT/Apache-2.0 | core | Standard UUID type, parsing and formatting. The daemon will enable the `v7` feature to generate ids. |
| `proptest` *(dev)* | MIT/Apache-2.0 | tests | Property testing with automatic shrinking of failing cases. |
| `serde_json` *(dev)* | MIT/Apache-2.0 | tests | Checks that serialized layouts load back unchanged. |

### Planned (approve when first added)

| Crate | License | Purpose |
|---|---|---|
| `tokio` | MIT | Async runtime (daemon, client) |
| `tracing`, `tracing-subscriber` | MIT | Structured logging |
| `anyhow` | MIT/Apache-2.0 | Errors in binaries |
| `clap` | MIT/Apache-2.0 | CLI parsing |
| `prost`, `protox` | Apache-2.0 / MIT/Apache-2.0 | Protobuf types and a pure-Rust schema compiler |
| `portable-pty` | MIT | PTYs on Unix and Windows ConPTY |
| `alacritty_terminal` | Apache-2.0 | VT emulation (behind the `Emulator` trait) |
| `rusqlite` (bundled) | MIT (SQLite: public domain) | Persistence |
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
| Per-OS data directories | `platform/` (planned) | A few env-var lookups don't need a dependency. |
