# 0006. All-Rust graphical app: winit + wgpu + an in-house widget toolkit

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
Urushi is a **graphical app that contains terminals**, not a TUI. It must run on macOS, Linux, Windows, iOS and Android with native performance. Tauri, Electron and webviews are excluded. Dependencies must be permissive.

## Decision
- **Rust everywhere.**
- **Windowing and input:** winit. **GPU:** wgpu (Metal/Vulkan/DX12/GLES). **Text:** cosmic-text. **Accessibility:** AccessKit.
- **Our own widget toolkit (`urushi-ui`):** Elm architecture (a pure `update`, plus a `view` that builds a widget tree), laid out into a renderer-agnostic `Scene`. Only `urushi-render` touches the GPU.
- Terminal panes are drawn as ordinary quads and text in the same scene.

## Alternatives considered
- **Flutter + Rust core:** mature on mobile, but adds a second language and a framework we don't control.
- **A separate native UI per OS** (SwiftUI, Compose, WinUI and more): roughly four UI codebases, and Linux has no permissively licensed native toolkit (GTK is LGPL).
- **Qt:** LGPL/GPL or commercial. **Slint:** GPL or a custom royalty-free license, not OSI-permissive.
- **egui / iced:** permissive, but immediate-mode or desktop-first, with weak mobile support. We would still need to work around them for the terminal surface.

## Consequences
- We own the widget set (sidebar, tabs, splits, buttons, menus, modals, text input, forms). Keeping it small is a design goal.
- UI is testable without a GPU through `Scene` snapshots.
- Mobile soft-keyboard and IME handling need small platform glue modules.
