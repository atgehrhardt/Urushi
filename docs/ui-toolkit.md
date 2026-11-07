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
- **Content:** `Label` (truncated with an ellipsis), `Field` (text input with caret), `Paint` (a custom painter, used for terminal panes).
- **Interaction:** `on_click(msg)`, `key(k)` for identity, `cursor(icon)`, `scroll(key, offset)`, and `role(role, label)` for accessibility.
- **Widgets** (`widgets.rs`): `button`, `icon_button`, `text_field`, `toggle`, `divider`, `modal`. All styling comes from `Theme` tokens (`theme.rs`).

## Scene (`scene.rs`)
- A flat, ordered display list: `Quad`, `Text`, `PushClip`, `PopClip`, in logical pixels.
- `Scene::dump()` is a stable text form. Assert on it in tests instead of on pixels.

## Text (`text.rs`)
- `TextSystem` wraps cosmic-text with the bundled fonts: Inter for UI, JetBrains Mono for terminals (OFL-1.1, `assets/fonts/`). System fonts are added as fallbacks.
- Layout uses it to measure. The renderer uses the same instance to rasterize.

## Accessibility (`access.rs`)
- Nodes with a `role` become AccessKit nodes (labels, bounds, click actions).
- The runner forwards AccessKit actions as clicks.
- macOS VoiceOver and UI scripting can see and press every control.

## Renderer (`urushi-render`)
- **Two instanced pipelines** (`shaders.wgsl`): SDF rounded and bordered quads, and glyphs from a 2048² RGBA atlas packed in shelves (`atlas.rs`).
- **Batching:** consecutive primitives of the same kind under the same clip become one draw. Clips become scissor rects.
- **Text** is shaped at physical size and cached per (text, font, size).
- **Targets:** `SurfaceRenderer` draws into a window. `Offscreen` draws into memory for pixel tests (`tests/pixels.rs` runs on the real GPU and skips where none exists).

## Terminal panes (`urushi-app/src/terminal.rs`)
- Pane rectangles come from `urushi_core::Layout::geometry`, with room left for dividers.
- Each cell is drawn on the grid. Backgrounds, selection and cursor are painted first, then glyphs.
- Divider dragging maps to `MoveDivider`. Grid sizes map to `SetPaneSize`.

## Limitations and follow-ups
- No IME composition preview on desktop yet. Committed text works on mobile.
- No animations.
- One window per app instance.
