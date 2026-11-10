# 0019. Draw frosted glass in our own renderer, with a backdrop-blur pass

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
The UI was redesigned as dark frosted glass: translucent panels over a softly glowing
backdrop, and overlays (dialogs, menus, the task board, drag ghosts) that blur what they
cover. The renderer had only opaque instanced quads and glyphs in a single pass
(ADR 0006). It runs on macOS, Linux, Windows, iOS and Android, and the device is
requested with `downlevel_webgl2_defaults`, so compute shaders aren't available
everywhere. UI is tested on `Scene::dump()` and offscreen pixel tests.

## Decision
- **The scene gains two kinds of primitive.**
  - Quads get a feathered edge (`softness`) and a vertical gradient. Shadows and the
    backdrop glows are analytic soft quads, with no extra passes.
  - A new **`Primitive::Backdrop`** means "blur what is already drawn under this rounded
    rectangle". `Node::backdrop_blur` emits one under a node's fill.
- **How the renderer draws a frame with backdrops** (`urushi-render/src/blur.rs`):
  1. The frame is drawn into an offscreen texture.
  2. At each backdrop the scene pass ends, and a dual-Kawase chain of fragment-shader
     passes blurs the texture: halve the resolution up to five times, then double back
     to half resolution.
  3. A new pass draws the blurred copy inside the backdrop's SDF shape and carries on.
  4. At the end the texture is copied to the target.

  A frame without backdrops is still one pass straight into the target.
- **Backdrops only on overlays.** Persistent chrome (sidebar, tab bar, terminals) sits
  only over the smooth glows, which a blur wouldn't change. It uses tint alone, so
  ordinary frames cost what they did before. A dialog's scrim blurs the whole window
  once, and the dialog card adds no second blur.
- **Glass tokens in `Theme`.** These are `glows`, `glass`/`glass_low`, `glass_strong`,
  `glass_border`, `well`, `shadow`, `blur` and `radius_large`, plus the
  `widgets::panel` / `widgets::overlay` helpers. Screens never hard-code glass colours.

## Alternatives considered
- **OS vibrancy** (macOS `NSVisualEffectView`, Windows Acrylic, via `window-vibrancy`).
  - It needs a transparent window, a non-opaque surface alpha mode, and per-OS code for
    each platform.
  - Linux compositors and the mobile shells have no equivalent, so we'd need the in-app
    look anyway.
  - It only blurs the *desktop* behind the window, not our own terminals under a dialog,
    which is where frost matters.
  - Offscreen pixel tests can't see it.
- **Tint only, no blur.** It's cheap, but text in terminals under a translucent dialog or
  menu stays sharp and competes with the overlay. That isn't frosted glass.
- **A Gaussian or compute-shader blur.** A separable Gaussian with a large radius needs
  many taps per pixel at full resolution. Compute isn't guaranteed under the downlevel
  limits we request. Dual Kawase gives a wide, smooth blur in a few cheap passes at
  reduced resolution.

## Consequences
- The renderer owns a second shader module and an intermediate texture plus five
  half-size levels, recreated when the target's size changes. Frames with overlays cost
  a few extra full-screen passes.
- A backdrop in a scene splits batching into segments. Tests cover blurred-inside and
  sharp-outside (`tests/pixels.rs`).
- Terminal backgrounds are translucent, so the terminal painter draws one sheet behind
  all panes and paints cells "in the background colour" opaque.
