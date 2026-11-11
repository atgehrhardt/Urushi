# 0046. Show one board column at a time on phones, switched by tabs and swipes

- **Status:** Accepted
- **Date:** 2026-11-11

## Context
On a phone (`AppState::compact`, windows under 700 logical pixels) the task board drew
every column side by side, as on a desktop. At an iPhone's 393 pt, three columns left
each card about 60 pt wide: titles read "Retr…", tags and badges were clipped, and the
header (board tabs, Rename/Delete or Sync now/Remove, ×) ran past the edge, so a Jira
board's close button was off screen (UR-58). The toolkit only scrolls vertically, and a
finger that moves scrolls, so nothing off to the side could be reached.

## Decision
- A phone shows **one column at a time**, across the whole width (`view/board/phone.rs`).
- Under the filter bar, a **tab per column** shows its count and name; tapping one shows
  it. A **sideways swipe** on the board steps to the next or previous column
  (`touch.rs` reports `TouchAction::Swipe` for a finger that sets off sideways and travels
  `SWIPE`; `Msg::Swipe` acts only on a phone's open board with nothing over it).
- Which column is shown is client state (`BoardPanel::column`); another board starts at
  its first column.
- A card moves to another column by long-press-dragging it **onto that column's tab**
  (the tabs are drop targets while a card is dragged), or with the editor's Column.
- The header is a **board picker** ("Name ▾", `MenuTarget::Boards`), the board's **⋯**
  (`MenuTarget::Board`, which on a phone also holds Rename/Delete or the sync state,
  Sync now and Remove) and ×.
- Desktops are unchanged.

## Alternatives considered
- **Stacked sections** (one vertical list, a collapsible header per column): reuses
  vertical scrolling and in-list drag, but a long column (often Done) pushes the rest
  far down, and it reads as a list rather than the board people know from Jira's app.
  The user chose columns.
- **Columns side by side, scrolling sideways:** needs horizontal scrolling and paging in
  the toolkit, and a sideways finger would fight with card drags.
- **Keep the desktop layout, narrower:** no width makes 3–6 columns readable on a phone.

## Consequences
- Cards on phones have the room for the Jira key, tags and thread chip again.
- A horizontal finger movement no longer scrolls anything: it is a swipe, ignored
  everywhere but a phone's board.
- Dragging a card within the shown column, or onto a tab, works; the column under the
  finger doesn't change while dragging.
- Narrow desktop windows (under 700 pt) get the same layout, which has no swipe without a
  touch screen; the tabs still switch columns.
