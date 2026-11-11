# 0047. Give board columns a minimum width and scroll the board sideways, rather than shrink them

- **Status:** Accepted
- **Date:** 2026-11-11

## Context
Wider than a phone (700 pt and up), the board drew every column side by side and shared
the panel's width among them, however little there was. On an unfolded Galaxy Fold 6
(about 760 pt, less the sidebar) or a narrow desktop window, a Jira board's columns came
out around 120 pt wide, which is too cramped to read. Its header (tabs, sync state, Sync now, Open in
Jira, Remove, ×) and filter chips also ran past the panel's edge, hiding ×. The toolkit
could only scroll vertically. Phones already show one column at a time (ADR 0046).

## Decision
- The toolkit gains sideways scrolling: `Node::scroll_x(key, offset)` makes a clipped row
  that scrolls, and `Node::min_width(w)` keeps a `Fill` child in a row at least `w`
  wide. Unlike a scrolling column, a sideways row's `Fill` children still share the
  width there is, and scroll only for their minimums. `ScrollRegion` has an `axis`, and
  `Frame::scroll_region_along` finds the innermost region scrolling a given way.
- Board columns share the panel's width but never get narrower than
  `BOARD_COLUMN_MIN_WIDTH` (a dragged card's width plus the column's padding); past that the board
  scrolls sideways. The board tabs and the filter chips also scroll sideways
  instead of running off the edge.
- Input: a trackpad's sideways movement, Shift with a mouse wheel, and a finger moving sideways all
  scroll the row under them (`scrolling.rs`, `touch.rs`). A dragged card held near the
  left or right edge scrolls the board that way, so it can reach a column out of view.
- When the panel is narrower than 880 pt (`view::board::narrow`), the header's actions
  go behind **⋯** and the filter bar keeps only the search and **Filters ▾**, as on a
  phone.

## Alternatives considered
- **Keep shrinking columns:** this is what was cramped.
- **The phone layout (one column) up to a larger width:** this wastes a foldable's width,
  which fits more than one readable column. The user asked for sideways scrolling.
- **Wrap columns onto a second row:** this breaks the board's left-to-right order and its
  full-height columns.
- **A real horizontal scrollbar:** the toolkit draws none for vertical lists either, and
  the half-visible next column already shows there is more.

## Consequences
- Every sideways row now needs the runner to route horizontal input to it. Vertical
  scrolling looks only for vertical regions, so a wheel over the gap between columns no
  longer reaches the row.
- A sideways finger movement no longer reaches a phone's board as a swipe when it starts
  over a sideways row; the phone board has none.
- Edge scrolling during a drag happens only as the pointer moves; holding still at the
  edge doesn't keep scrolling.
