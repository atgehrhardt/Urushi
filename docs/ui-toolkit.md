# UI toolkit (`urushi-ui`) and renderer (`urushi-render`)

Urushi draws its own GUI with the GPU (ADR 0006). This page is the map.

## Elm architecture
```
AppState ──view()──► Node<Msg> tree ──layout::build()──► Frame { scene, hits, scroll_regions, access }
    ▲                                                          │
    └──────── update(msg) → Vec<Effect> ◄── runner maps OS input to Msg via frame.click(p) …
```
- `urushi-app/src/update.rs` is the only place state changes. It's pure and unit-tested (`update/tests.rs`).
- `view/` builds a fresh `Node` tree every frame. Building is cheap, and long lists build only what can show (the board's columns, `view/board/column.rs::window`). Layout isn't cached; text widths and shaped lines are (`TextCache`, below). Views that would redo real work each frame keep a memo checked against its inputs (`filter/cache.rs`, ADR 0048).
- The runner redraws after every message, every daemon event and each animation step. A pointer move alone redraws only when `Frame::hover_changes` says a clickable or droppable region was entered or left (ADR 0048). It logs each frame's view, layout and render times at `debug` (`frame`).

## Nodes and layout (`node.rs`, `layout.rs`)
- **`Flow`:** `Row` / `Column` / `Stack` (layers, for dialogs).
- **Sizes:** `Length::Fixed | Fill(weight) | Fit`. A simplified flexbox in two passes: measure, then place. `max_height(h)` caps a `Fit` height; with `scroll` the rest is reached by scrolling (the task editor's body, so its buttons stay on screen). `min_width(w)` keeps a `Fill` child in a row at least `w` wide; in a `scroll_x` row the rest is reached by scrolling sideways (the board's columns, ADR 0047). A child of a scrolling box that lies wholly outside its clip isn't painted, hit-tested or made accessible (its ids are still counted), so a long list costs what it shows; content spilling out of such a child isn't drawn while it is out of sight (ADR 0048).
- **Content:** `Label` (truncated with an ellipsis), `Field` (text input with caret and selection), `Image` (a bitmap, e.g. the logo; `Node::image`), `Paint` (a custom painter, used for terminal panes).
- **Interaction:** `on_click(msg)`, `on_secondary_click(msg)` (right / two-finger click), `on_drag(msg)` and `on_drop(msg)` (the runner starts a drag after 5 px of movement; a drop target shows its hover background under the pointer), `key(k)` for identity, `cursor(icon)`, `scroll(key, offset)` and `scroll_x(key, offset)` (each leaves a `ScrollRegion` with its `Axis`; `Frame::scroll_region_along` finds the innermost one scrolling a given way, so a sideways wheel scrolls the row a column sits in), and `role(role, label)` for accessibility. `Frame::click`, `secondary_click`, `drag_source` and `drop_target` find the topmost handler at a point, looking no further down than the first `opaque()` node: a dialog card is opaque, so a click on its own blank space never reaches the backdrop under it, whose click closes the dialog.
- **Positioned layers:** inside a `Stack`, `at(point)` places a child at its natural size at that point, moved back inside the stack if it would overflow. Context menus and the drag label use it.
- **Sliding:** `slide_x(fraction)` shifts a node and its children left by that fraction of its own width; `slide_y(fraction)` shifts them up by that fraction of its height (the quick terminal drops down with it, `view/quick_terminal.rs`). In a clipped parent, animating it from 1 to 0 slides a panel in (the task board, `view/mod.rs::with_board`, and the sidebar, `view/mod.rs::sidebar_slot`). The runner drives such animations by redrawing until done (`runner.rs::animate`).
- **Glass:** `gradient(bottom)` fades the background downwards, `shadow(Shadow)` casts a soft shadow under the box, and `backdrop_blur(px)` frosts what's drawn under it (ADR 0019). Keep backdrops to overlays: each costs a few GPU passes.
- **Widgets** (`widgets.rs`): `button`, `icon_button`, `sidebar_button` (an icon drawn from boxes), `panel` (glass resting on the backdrop) and `overlay` (blurring glass with a shadow), `text_field`, `text_area` (several lines, a fixed number tall), `wrapped_text` (text wrapped to a fixed box, ending in an ellipsis when it doesn't fit), `toggle`, `divider`, `modal` (and `modal_with_header`, headed by any node, such as the task editor's breadcrumb), and the selector pair `select_button` + `select_options` (an inline option list that scrolls past a set number of rows). All styling comes from `Theme` tokens (`theme.rs`).

## Text fields (`textfield.rs`, `field.rs`)
- **`TextField`** is the pure editing model: text, caret, a selection anchor, and a bounded undo/redo history. Edits replace the selection; moves take `extend` (Shift). It has word moves and deletes, `select_word` (double click), `select_all`, `cut`, and `place(offset, extend)` for clicks. `TextField::new` is single-line (pasted line breaks become spaces); `TextField::multiline` keeps `\n`, and `newline`, `up`/`down` and `line_start`/`line_end` work on its lines (counted in characters between `\n`s; the model knows nothing of fonts, so Up/Down move by logical lines, not wrapped ones).
- **Painting** (`field.rs`): the selection is drawn behind the text. A single-line field scrolls sideways to keep the caret in view; a multiline one wraps to the box (`TextSystem::wrap`) and scrolls up instead. Each painted field leaves a `FieldRegion` in `Frame::fields` (its widget's key, text area, its lines as byte ranges, and where the text's origin is after scrolling).
- **Pointer → caret:** `Frame::field_at(p)` finds the visible field under the pointer and `caret_at(text_system, region, p)` the nearest character boundary on the line under `p` (a click past a wrapped line's end stays on that line). The runner uses them for click-to-place, drag-to-select (re-finding the field by key with `Frame::field`) and double/triple clicks.

## Scene (`scene.rs`)
- A flat, ordered display list: `Quad`, `Text`, `Image`, `Backdrop`, `PushClip`, `PopClip`, in logical pixels. An `Image` carries an `Arc<ImageData>` (straight RGBA plus an id for caching).
- A `Quad` can have a feathered edge (`softness`, centred on its outline: shadows and glows) and a vertical `gradient`. A `Backdrop` blurs what's already drawn under its rounded rectangle.
- `Scene::dump()` is a stable text form. Assert on it in tests instead of on pixels.

## Theme (`theme.rs`)
- One dark theme: frosted glass. `Theme::with_scheme(background, glows, accent)` recolors its backdrop and accent (the app's color schemes); the glass is tinted towards the background. Tokens cover the backdrop (`background`, `glows`), glass (`glass`/`glass_low` for panels, `glass_strong` for overlays, `glass_border`, `well` for inputs, `shadow`, `blur`, `radius_large`), interaction states (translucent `hover`, `selected`, `border`), text, status colours, labels and the terminal palette (its background is translucent; the terminal painter draws it once under all panes).

## Text (`text.rs`)
- `TextSystem` wraps cosmic-text with the bundled fonts: Inter for UI, JetBrains Mono for terminals (OFL-1.1, `assets/fonts/`). System fonts are added as fallbacks. `TextSystem::wrap` breaks text into lines for a width (at `\n`, then between words, then inside a word too long for a line) from its own cached measurements, so fields and `wrapped_text` agree on where lines break. `TextSystem::wrap_first` stops after a number of lines: `wrapped_text` wraps only the lines its box shows (plus one, to know whether to end in an ellipsis). A card's long Jira description at a phone's narrow column width would otherwise be measured, often character by character, on every frame.
- Layout uses it to measure. The renderer uses the same instance to rasterize.
- **Caches** (`text_cache.rs`): widths, and the renderer's shaped lines, are kept in a `TextCache` of two generations. When the newer one is full the older is dropped, and a hit in the older moves forward, so a screen with more text than one generation holds keeps what it used last frame instead of losing everything at once (ADR 0048). Lookups borrow the text. `truncate` searches only as many characters as could fit (`TextSystem::most_chars`), so a long text is never measured whole. `TextSystem::shaped_count` counts cache misses, for tests.

## Accessibility (`access.rs`)
- Nodes with a `role` become AccessKit nodes (labels, bounds, click actions).
- The runner forwards AccessKit actions as clicks.
- macOS VoiceOver and UI scripting can see and press every control.

## Renderer (`urushi-render`)
- **Two instanced pipelines** (`shaders.wgsl`): SDF rounded and bordered quads (with soft edges and gradients), and glyphs from a 2048² RGBA atlas packed in shelves (`atlas.rs`). Images go in the same atlas as colour entries, box-filtered to the exact pixel size they're drawn at, once per (image, size) (`image.rs`).
- **Backdrops** (`blur.rs`, `blur.wgsl`): a frame with any is drawn into an offscreen texture. Each backdrop ends the scene pass, blurs the texture with dual Kawase (halving up to five times, then back to half size), and starts the next pass by drawing the blurred copy in its shape. The texture is then copied to the target. Frames without backdrops stay a single pass.
- **Batching:** consecutive primitives of the same kind under the same clip become one draw (a backdrop is always its own). Clips become scissor rects (`scissor.rs`), rounded outwards and clamped to the render target. A clip with no area draws nothing. wgpu panics on a scissor outside the target, and at fractional scales (Android's 2.625) rounding the edges of an empty clip lying off screen used to make one (UR-55).
- **Text** is shaped at physical size and cached per (text, font, size) in a `TextCache`.
- **Targets:** `SurfaceRenderer` draws into a window. `Offscreen` draws into memory for pixel tests (`tests/pixels.rs` runs on the real GPU and skips where none exists).

## Terminal panes (`urushi-app/src/terminal.rs`)
- Pane rectangles come from `urushi_core::Layout::geometry`, with room left for dividers.
- Each cell is drawn on the grid. Backgrounds, selection and cursor are painted first, then glyphs.
- Divider dragging maps to `MoveDivider`. Grid sizes map to `SetPaneSize`.

## Limitations and follow-ups
- No IME composition preview on desktop yet. Committed text works on mobile.
- One window per app instance.
