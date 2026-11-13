# 0058. Write thread notes in Markdown, styled as they are typed, with a toolbar and a color of their own

- **Status:** Accepted
- **Date:** 2026-11-13
- **Supersedes:** the "notes are plain text" part of [0048](0048-thread-unread-notes-and-project-colors-in-the-daemon.md)

## Context
UR-72 asks for two things:
- notes with **formatted entry and format controls**;
- notes shown on the sidebar's thread cards in **colored text** rather than white.

ADR 0048 made notes plain text, stored by the daemon in `Thread.notes`.

The toolkit draws every text primitive in one font and one color. Its text field edits a plain string. Nothing in the GUI drew formatted text. Jira descriptions are already Markdown: the daemon converts them to and from ADF, and the app edits them raw.

## Decision
- **Notes are Markdown, stored as typed.** `Thread.notes` stays a string, so the wire, the store and older clients don't change. Older clients show the source.
- **One parser, in `urushi-core::markdown`.** It is pure and works one line at a time:
  - blocks: headings, `-`/`*`/`+` and `1.` items, `- [ ]` tasks, `>` quotes;
  - inline: `**bold**`, `_italic_`/`*italic*`, `~~strike~~`, `` `code` `` and links (`[text]` followed by the address in parentheses), with the same rules as the daemon's ADF converter.
  It returns byte ranges of the source, with the markup itself as `marker` spans. The editor draws the source with the markers dimmed, and the card leaves them out. Both come from one parse.
- **The editor is the source, styled as it is typed.** Bold text is semibold, italic is Inter Italic, code is mono, links are accent and underlined, struck text and done tasks are struck through, and the marks are muted.
  - A toolbar (B, I, S, `<>`, H, •, 1., ☐, ↗) and shortcuts (Cmd/Ctrl+B, I, E, K, Shift+X) apply `markdown::edit` functions. Each one returns an `Edit` (range, text, selection), which the field applies as one undo step (`TextField::replace`).
  - Enter continues a list, and ends it on an empty item.
- **The toolkit learns styled runs, not Markdown.**
  - `urushi_ui::styled` gives a text runs of fonts and colors, with strikethrough and underline.
  - A field takes optional runs. It wraps, draws and hit-tests by run, so the caret sits where the glyphs are.
  - `rich_line` draws one styled line and cuts it off with an ellipsis.
  - All runs share one size, so line height doesn't change. Headings are bold, not larger.
- **Bundle Inter Italic and Inter SemiBold Italic** from the same Inter 4.1 release as the other faces (OFL-1.1).
- **Notes have a color: `Thread.notes_color`** (a board-palette `LabelColor`, protocol 1.32).
  - It is set with `UpdateThread.notes_color` in the same all-or-nothing update as the notes.
  - Unset means the clients' default, yellow (a sticky note).
  - The daemon drops the color whenever a thread has no notes.
  - The editor shows the palette with the current color ringed, and Save sends notes and color together.
  - Cards draw the first line that has text in that color: formatted, markers removed, a task's box drawn before it. The note mark beside the title is drawn in the same color.

## Alternatives considered
- **True WYSIWYG (marks hidden while typing).** This needs a rich-text model in the toolkit: styled storage, caret moves across invisible marks, and list behavior. It is far more code and risk than styled source. Markdown source also stays readable in the CLI and on older clients.
- **A plain editor with a Preview tab.** This needs the least toolkit work, but you write raw markup and see the result only on another tab. The styled-source editor costs one generic toolkit feature (styled runs) that other text can reuse.
- **Storing formatting out of band** (spans beside the text, or HTML). This needs a protocol shape for rich text and a converter for the CLI and older clients, and it gains nothing over Markdown, which Jira descriptions already use.
- **Coloring notes with the project's color, or the accent.** This needs no new data, but a thread without a project would have no color, and the user asked to pick one. A per-thread palette color follows ADR 0048's project colors and suits both themes.
- **A Markdown crate (pulldown-cmark).** It parses whole documents into events, not source ranges per line with markers. We need about 300 lines, which mirror the daemon's converter (see `docs/dependencies.md`).
- **Larger headings.** Mixed line heights would complicate wrapping, scrolling and caret placement in the field. Semibold is enough to tell a heading apart in a note.

## Consequences
- Notes look the same on every client of 1.32 or later. Older clients show the Markdown source and never send a color. Older daemons get no color row, but formatting still works because it is only text.
- `urushi-core::markdown` and the daemon's `jira/adf/from_md.rs` share rules but not code. A change to one should be mirrored in the other.
- The field's styled path measures per run, which costs a few more cached measurements than one font. Plain fields (no runs) are measured whole, exactly as before.
- Fenced code blocks and rules read as plain text, because the parser works one line at a time.
- Two more font files ship (~0.8 MB).
