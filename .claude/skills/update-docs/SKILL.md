---
name: update-docs
description: Create or update Urushi's documentation as part of finishing any change — feature docs, architecture overview, ADRs, protocol and dependency docs, module docs, and the docs index. Use before declaring any task done, and whenever behaviour, architecture, a decision, the protocol, or dependencies change.
---

# Updating documentation

Documentation is part of the change, not a follow-up. A task isn't done until the docs match the code and `make check-docs` passes.

## Decide what to update
| You changed… | Update |
|---|---|
| User-visible behaviour, or added a capability | `docs/features/<feature>.md`. Create it from `docs/features/_template.md` if it doesn't exist. Set **Status**. |
| Which crate owns what, a data flow, or a boundary | `docs/architecture.md` (and the crate map in `AGENTS.md`). |
| A choice between real alternatives, a reversal, or an exception to an invariant | A new ADR (`write-adr` skill). Never rewrite an accepted ADR; supersede it. |
| Anything in `proto/` | `docs/protocol.md` (`change-protocol` skill). |
| Dependencies, tools or bundled assets | `docs/dependencies.md` (`add-dependency` skill). |
| A module's responsibility or invariants | Its `//!` module doc. Public items get `///` docs (the `missing_docs` lint enforces this). |
| Added, renamed or removed any doc file | The index in `docs/README.md`. |

## How to write them
- **Describe what is true now.** Use present tense and name the real modules, types and messages, so a reader can jump straight to the code.
- **Explain why.** The code already shows how. Link the ADR that holds the reasoning instead of repeating it.
- **Keep invariants next to where they're enforced.** Name the type or function that checks each one, and the test that proves it.
- **Record limitations honestly** under "Limitations and follow-ups", with enough context for someone to pick them up.
- **Be brief.** Prefer a table or list over paragraphs. Delete stale text; don't just append.

## Check
```sh
make check-docs   # every doc is indexed, every relative link resolves, every crate is documented
```
