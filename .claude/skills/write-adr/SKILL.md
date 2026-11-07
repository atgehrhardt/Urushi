---
name: write-adr
description: Record an architecture decision for Urushi. Use when choosing or reversing a technology, crossing a crate boundary rule in AGENTS.md, adding a license exception, adding a top-level crate, or making any choice a future contributor would ask "why?" about.
---

# Writing an ADR

ADRs live in `docs/adr/`, one decision per file, numbered in order.

## When an ADR is needed
- Adopting, replacing or removing a major dependency or external tool.
- Adding a top-level crate, or changing which crate owns which responsibility.
- Any exception to an AGENTS.md invariant or the license policy.
- A breaking protocol version (`v2`).
- Reversing an earlier ADR.

Small, local design choices don't need one. Explain them in module docs instead.

## Steps
1. Copy `docs/adr/0000-template.md` to `docs/adr/NNNN-short-kebab-title.md`, using the next unused number.
2. Fill in Context, Decision, Alternatives considered (with the *specific* reason each lost) and Consequences, including the new maintenance burden. Keep it to about a page.
3. Status starts as `Proposed`. It becomes `Accepted` once the user agrees, or immediately if the user asked for the change.
4. When superseding: set the old ADR's status to `Superseded by [NNNN](…)` and link back from the new one. Never delete ADRs.
5. Update anything the decision affects: `AGENTS.md`, `docs/architecture.md`, `docs/dependencies.md`.
