# 0008. Persist the workspace as one atomically-written JSON document

- **Status:** Accepted
- **Date:** 2026-11-06
- **Amended by:** [0053](0053-save-the-workspace-from-one-coalescing-saver.md) (one saver writes it, coalescing changes)

## Context
The daemon must persist projects, threads (with private facts such as the branches that existed before each thread), tabs, layouts and settings. The original plan said SQLite. In practice the whole workspace is small: kilobytes even with thousands of threads. It changes at human speed (a click, a split), and it's always read in full at startup.

## Decision
- The `Store` trait has one implementation, `JsonFileStore`: `<home>/workspace.json`, rewritten on every change via temp file, fsync and rename.
- The document carries a `format` version. Newer formats are refused, so an older daemon never overwrites them.
- Saves are ordered by revision, so a slow save can't overwrite a newer one.

## Alternatives considered
- **SQLite via `rusqlite` (bundled):** robust, but it compiles a large C library into the daemon (cross-compilation friction on Windows and ARM). Its query and migration machinery buys nothing for a document this size.
- **An embedded Rust key-value store (redb, sled):** more moving parts than the problem needs.

## Consequences
- No C dependencies in the daemon. The store is about 150 lines we own, and the file is easy to inspect and back up.
- If the workspace ever grows large, or needs partial updates (e.g. persisted scrollback), add a second `Store` implementation and an ADR superseding this one.
