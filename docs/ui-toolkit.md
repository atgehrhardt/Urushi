# UI toolkit (`urushi-ui`) and renderer (`urushi-render`)

Urushi draws its own GUI with the GPU (ADR 0006). This page is the map.

## Elm architecture
```
AppState ──view()──► Node<Msg> tree ──layout::build()──► Frame { scene, hits, scroll_regions, access }
    ▲                                                          │
    └──────── update(msg) → Vec<Effect> ◄── runner maps OS input to Msg via frame.click(p) …
```
- `urushi-app/src/update.rs` is the only place state changes. It's pure and unit-tested (`update/tests.rs`).
- `view/` builds a fresh `Node` tree every frame. Building is cheap; layout and text measurement are cached.

## Nodes and layout (`node.rs`, `layout.rs`)
- **`Flow`:** `Row` / `Column` / `Stack` (layers, for dialogs).
- **Sizes:** `Length::Fixed | Fill(weight) | Fit`. A simplified flexbox in two passes: measure, then place.
- **Content:** `Label` (truncated with an ellipsis), `Field` (text input with caret and selection), `Image` (a bitmap, e.g. the logo; `Node::image`), `Paint` (a custom painter, used for terminal panes).
- **Interaction:** `on_click(msg)`, `on_secondary_click(msg)` (right / two-finger click), `on_drag(msg)` and `on_drop(msg)` (the runner starts a drag after 5 px of movement; a drop target shows its hover background under the pointer), `key(k)` for identity, `cursor(icon)`, `scroll(key, offset)`, and `role(role, label)` for accessibility. `Frame::click`, `secondary_click`, `drag_source` and `drop_target` find the topmost handler at a point.
- **Positioned layers:** inside a `Stack`, `at(point)` places a child at its natural size at that point, moved back inside the stack if it would overflow. Context menus and the drag label use it.
- **Sliding:** `slide_x(fraction)` shifts a node and its children left by that fraction of its own width. In a clipped parent, animating it from 1 to 0 slides a panel in (the task board, `view/mod.rs::with_board`, and the sidebar, `view/mod.rs::sidebar_slot`). The runner drives such animations by redrawing until done (`runner.rs::animate`).
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
- One dark theme: frosted glass. Tokens cover the backdrop (`background`, `glows`), glass (`glass`/`glass_low` for panels, `glass_strong` for overlays, `glass_border`, `well` for inputs, `shadow`, `blur`, `radius_large`), interaction states (translucent `hover`, `selected`, `border`), text, status colours, labels and the terminal palette (its background is translucent; the terminal painter draws it once under all panes).

## Text (`text.rs`)
- `TextSystem` wraps cosmic-text with the bundled fonts: Inter for UI, JetBrains Mono for terminals (OFL-1.1, `assets/fonts/`). System fonts are added as fallbacks. `TextSystem::wrap` breaks text into lines for a width (at `\n`, then between words, then inside a word too long for a line) from its own cached measurements, so fields and `wrapped_text` agree on where lines break.
- Layout uses it to measure. The renderer uses the same instance to rasterize.

## Accessibility (`access.rs`)
- Nodes with a `role` become AccessKit nodes (labels, bounds, click actions).
- The runner forwards AccessKit actions as clicks.
- macOS VoiceOver and UI scripting can see and press every control.

## Renderer (`urushi-render`)
- **Two instanced pipelines** (`shaders.wgsl`): SDF rounded and bordered quads (with soft edges and gradients), and glyphs from a 2048² RGBA atlas packed in shelves (`atlas.rs`). Images go in the same atlas as colour entries, box-filtered to the exact pixel size they're drawn at, once per (image, size) (`image.rs`).
- **Backdrops** (`blur.rs`, `blur.wgsl`): a frame with any is drawn into an offscreen texture. Each backdrop ends the scene pass, blurs the texture with dual Kawase (halving up to five times, then back to half size), and starts the next pass by drawing the blurred copy in its shape. The texture is then copied to the target. Frames without backdrops stay a single pass.
- **Batching:** consecutive primitives of the same kind under the same clip become one draw (a backdrop is always its own). Clips become scissor rects.
- **Text** is shaped at physical size and cached per (text, font, size).
- **Targets:** `SurfaceRenderer` draws into a window. `Offscreen` draws into memory for pixel tests (`tests/pixels.rs` runs on the real GPU and skips where none exists).

## Terminal panes (`urushi-app/src/terminal.rs`)
- Pane rectangles come from `urushi_core::Layout::geometry`, with room left for dividers.
- Each cell is drawn on the grid. Backgrounds, selection and cursor are painted first, then glyphs.
- Divider dragging maps to `MoveDivider`. Grid sizes map to `SetPaneSize`.

## Limitations and follow-ups
- No IME composition preview on desktop yet. Committed text works on mobile.
- One window per app instance.
