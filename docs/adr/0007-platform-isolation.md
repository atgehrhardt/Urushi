# 0007. Isolate platform-specific code in `platform/` modules

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
We target five platforms. Scattered `#[cfg]` branches make code hard to read and hard to test, and porting gets more expensive each time.

## Decision
`#[cfg(target_os = …)]` and per-OS APIs are allowed only inside `platform/` modules, behind a platform-neutral interface. Everything outside them must compile on every target. CI runs `cargo check` for the iOS and Android targets, and full tests on macOS, Linux and Windows.

## Consequences
- Adding a platform means implementing the `platform/` interfaces, not editing domain code.
- `urushi-core` is the strictest case: no IO at all (see AGENTS.md).
