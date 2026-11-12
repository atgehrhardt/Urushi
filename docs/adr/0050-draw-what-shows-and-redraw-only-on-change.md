# 0050. Lay out and paint only what a scrolling list shows, keep text caches in two generations, and redraw on pointer moves only when hover changes

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
A Jira board with a few hundred issues (UR-66) lagged when scrolling, hovering, typing in
the search and choosing filters. The toolkit rebuilds the whole `Node` tree every frame
(ADR 0006), and that was fine for small screens. On a large board, though, every frame:

- built, laid out and painted every card, including the hundreds scrolled out of sight;
- measured more distinct strings than the text-width cache held (10,000), so the cache
  was wiped whole partway through each frame and most labels were shaped again by
  cosmic-text (the renderer's shaped-text cache did the same at 20,000);
- happened on every pointer move, even when nothing under the pointer changed;
- filtered every card from scratch, and walked the board ten times for the Filters menu.

On a 500-issue board this cost about 18 ms of view and layout per frame (release build),
before rendering.

## Decision
- **Paint only what a scrolling box shows.** `layout::place_children` skips a child of a
  `scroll`/`scroll_x` box that lies wholly outside its clip: no paint, no hit region, no
  accessibility node, no descent. Scroll extents come from sizes, so they are unchanged,
  and node ids still count skipped subtrees, so the ids of later nodes stay stable.
- **Build only the cards in sight.** A board column knows every card's height (cards are
  all the same size), so `view/board/column.rs::window` builds the cards that can show at
  the column's offset (bounded by the window's height, and covering wherever layout
  clamps the offset), plus one either side. Spacers stand in for the rest.
- **Two-generation text caches.** `TextCache` (in `urushi-ui`) keeps a newer and an older
  generation. When the newer is full, the older is dropped; a hit in the older moves the
  entry forward. Lookups borrow the text instead of allocating a key. Layout's widths and
  the renderer's shaped lines both use it. `truncate` searches only as many characters as
  could fit the width (`TextSystem::most_chars`), so a long text is never measured whole.
- **Redraw on a pointer move only when hover changes.** `Frame::hover_changes(from, to)`
  says whether any clickable or droppable region changes between containing and not
  containing the pointer. The runner redraws on `CursorMoved`/`CursorLeft` only then.
  Everything else a move does (drags, text selection, terminal hover) goes through a
  message, and every message redraws.
- **Memoize the filter.** `filter::cache::FilterCache` keeps which cards show and each
  field's values for the board on screen, checked against the board and the filter it
  was made from on every use, inside a `RefCell` so `view()` can fill it.

## Alternatives considered
- **Retained-mode widgets or a diffing tree:** this would rework every screen to fix one
  hot path, and give up the "view is a function of state" model (ADR 0006).
- **A larger text cache:** this moves the cliff without removing it. A board with long
  descriptions can always need more entries than any fixed size holds.
- **An LRU cache:** this keeps per-entry bookkeeping on every hit, the hottest path in
  layout. Two generations give the same "keep what was just used" result with plain maps.
- **Culling any node outside its parent's clip:** children may draw outside their own box
  (overlays, positioned menus), so only scroll containers, whose children stay in their
  slots, are safe to cull.
- **Keep the filter memo in `update()`:** tests and the snapshot tool build states
  directly and call `view()`, so a memo filled only in `update` could go stale without
  anyone noticing. Checking it on use can't.

## Consequences
- `Scene::dump()` and `Frame::access` hold only what is in sight. A check of content far
  down a long list has to scroll it (`snapshot::scenario_scrolled_to_end`). Screen readers
  likewise see only the cards in view, which is how lists scrolled by a window usually
  read.
- A scroll container's children must stay within their slots to be drawn: content that
  spills out of a child is not drawn while that child is out of sight.
- Anything new that changes a frame's look with the pointer must go through hit regions
  (`on_click`/`on_drop` plus `hover_background`) or a message, or it won't redraw.
- The filter memo compares the shown board on each use (cheap next to filtering it), and
  clones it when it changes.
