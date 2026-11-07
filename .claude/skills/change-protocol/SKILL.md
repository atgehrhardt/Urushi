---
name: change-protocol
description: Safely evolve Urushi's client/daemon wire protocol. Use whenever editing files in proto/, the frame codec in urushi-proto, or adding/removing a request, response, or event.
---

# Changing the wire protocol

Old clients (including mobile apps not yet updated) talk to new daemons and the other way round. **Within a major version, changes are additive only.** See ADR 0003 and `docs/protocol.md`.

## Rules
- **Allowed:** add new messages, new fields with **new** field numbers, new `oneof` variants, and new enum values (with a sensible fallback for unknown values on the receiving side).
- **Forbidden in `v1`:**
  - renumbering or reusing a field number
  - changing a field's type
  - renaming a package
  - removing a field without reserving it: `reserved 7; reserved "old_name";`
- **Every enum's value 0 is `*_UNSPECIFIED`**, and the code must handle it.
- **Unknown fields and variants must not crash the receiver.** Ignore them, or answer with an `Unsupported` error.
- **A new capability that clients need to detect** gets a minor-version bump in `Hello`/`Welcome`. Don't make clients infer capabilities by trial and error.
- **A breaking change means a new package `urushi.v2`** and a daemon that serves both versions. It also needs an ADR.

## Steps
1. Edit the `.proto` files under `proto/urushi/v1/`.
2. Run `buf lint` and `buf breaking --against '.git#branch=main'`. Both must pass.
3. Rebuild (`cargo build -p urushi-proto`). Code generation runs in `build.rs` via protox.
4. Add or extend a **round-trip test** (encode → decode → equal) for every new or changed message, in `urushi-proto`'s tests.
5. Handle the message in the daemon router and in `urushi-client`. Add a CLI command if it's useful for headless verification.
6. Update `docs/protocol.md`, including the minor version if it changed.
7. Run `make ci`.
