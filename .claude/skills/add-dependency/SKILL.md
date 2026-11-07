---
name: add-dependency
description: Vet and add (or upgrade) any external crate, tool, or asset in Urushi. Use before touching [workspace.dependencies], adding a dev-dependency, bundling a font or asset, or adopting a new build tool. Enforces the permissive-only license policy.
---

# Adding a dependency

Urushi builds in-house by default. Any outside code must have a **permissive, free-for-commercial-use license**. The policy and allowlist are in `docs/dependencies.md` and `deny.toml`.

## 1. Decide whether we need it at all
- If the functionality is about **300 lines or fewer** and well understood, write it ourselves. Record it in the "Written in-house instead" table in `docs/dependencies.md`.
- If it's needed by only one platform, it belongs in that crate's `platform/` module (ADR 0007).
- Prefer crates that are actively maintained, have few dependencies of their own, and let us turn off default features.

## 2. Check the license (including the crates it pulls in)
1. Read the crate's `license` field **and** its LICENSE file. If they disagree, the stricter one applies.
2. Add the crate to the root `Cargo.toml` under `[workspace.dependencies]`, with `default-features = false` unless you need them. Reference it from the member crate with `{ workspace = true }`.
3. Run `cargo deny check`. It must pass with **no new `exceptions`**.
4. If anything it pulls in is GPL, LGPL, AGPL, MPL, EPL, CDDL, SSPL, BUSL or carries a non-commercial clause, **stop**. Find an alternative or write it ourselves. Exceptions require an ADR (`write-adr` skill) and explicit approval from the user.

For non-Rust items (tools, fonts, assets): verify the license by hand. OFL-1.1 is allowed for fonts only.

## 3. Check it builds everywhere it's used
- If the crate is used by `urushi-core` or any client crate, run `make check-targets` (iOS + Android).
- Then run `make ci`.

## 4. Record it
Add a row to the right table in `docs/dependencies.md`: name, license, which crate uses it, and **why we didn't write it ourselves**. If it was in the "Planned" table, move it into "Approved".

## Upgrades
- A major version bump goes through steps 2–3 again. Licenses do change between versions.
- Keep a single version in the workspace. Investigate any duplicate versions `cargo deny` warns about.
