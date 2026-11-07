# Urushi

A cross-platform graphical workspace for working with AI agents. Projects contain threads. Every thread is its own git worktree, with tabs of split, recoverable terminals.

- **Platforms:** macOS, Linux, Windows (daemon + app), iOS and Android (remote clients over Tailscale)
- **Stack:** all Rust. In-house multiplexer, widget toolkit and wire protocol. GPU rendering via wgpu.
- **Licensing:** only permissively licensed dependencies (see `docs/dependencies.md`)

Start with [`AGENTS.md`](AGENTS.md) and [`docs/architecture.md`](docs/architecture.md).

```sh
make ci   # format check, clippy, tests, license audit
```
