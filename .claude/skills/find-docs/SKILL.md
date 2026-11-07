---
name: find-docs
description: Locate Urushi's existing documentation before starting any task — architecture, decisions (ADRs), feature docs, protocol, dependencies, and module docs. Use at the start of every feature, bug fix, refactor, or design question, and whenever you wonder "why is it like this?" or "where does X live?".
---

# Finding documentation

Read before you write code. The answer to "why is it like this?" is usually already written down. Contradicting an accepted decision by accident is the most expensive mistake you can make here.

## Where to look, in order
1. **`docs/README.md`**: the index of every doc. Start here.
2. **`docs/features/<feature>.md`**: if your task touches a user-visible capability, read its feature doc first. It names the crates, modules, protocol messages, invariants and tests involved.
3. **`docs/adr/`**: decisions and their reasons. Search for the technology or concept: `grep -ril "<term>" docs/adr`. Treat `Accepted` ADRs as binding. To change one, write a superseding ADR (`write-adr` skill).
4. **`docs/architecture.md`**: component boundaries and data flow.
5. **`docs/protocol.md`** for anything that crosses the client/daemon boundary. **`docs/dependencies.md`** before adding any library.
6. **Module docs** (`//!` at the top of each `lib.rs` / `mod.rs`): the responsibilities and invariants of that code. `cargo doc --open -p <crate>` renders them.

## Fast searches
```sh
grep -rn "<concept>" docs/                    # all prose docs
grep -rn "^//!" crates/<crate>/src | head      # module responsibilities
ls docs/features docs/adr
```

## If there's nothing
Note it. You will be expected to create the missing doc when you finish (`update-docs` skill).
