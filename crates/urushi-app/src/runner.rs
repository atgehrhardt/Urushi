//! The winit event loop: window, renderer, input routing, accessibility.
//!
//! The runner is deliberately thin. It turns OS events into [`Msg`]s, runs effects, and
//! draws `view(state)` on every redraw. All behaviour is in `update.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use accesskit_winit::Adapter;
use urushi_render::SurfaceRenderer;
use urushi_ui::{CursorIcon, Frame, Key, Point, Rect, TextSystem, Theme, build};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::backend::Backend;
use crate::keys::{KeyPress, Mods};
use crate::msg::{Effect, Msg};
use crate::platform::{self, Clipboard};
use crate::state::AppState;
use crate::{UserEvent, view};

/// Lines scrolled per wheel notch.
const LINES_PER_NOTCH: f32 = 3.0;
/// Windows narrower than this (logical pixels) use the one-column phone layout.
const COMPACT_WIDTH: f32 = 700.0;

pub(crate) struct Runner {
    pub(crate) state: AppState,
    pub(crate) theme: Theme,
    pub(crate) text: TextSystem,
    pub(crate) backend: Backend,
    pub(crate) proxy: EventLoopProxy<UserEvent>,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) renderer: Option<SurfaceRenderer>,
    pub(crate) access: Option<Adapter>,
    pub(crate) frame: Option<Frame<Msg>>,
    pub(crate) pointer: Option<Point>,
    pub(crate) mods: Mods,
    pub(crate) pressed_in_area: bool,
    pub(crate) wheel_remainder: f32,
    pub(crate) scroll: HashMap<Key, f32>,
    pub(crate) clipboard: Clipboard,
    pub(crate) safe_area: Option<crate::config::SafeAreaFn>,
}

impl Runner {
    fn scale(&self) -> f32 {
        #[allow(clippy::cast_possible_truncation, reason = "scale factors are small")]
        self.window
            .as_ref()
            .map_or(1.0, |w| w.scale_factor() as f32)
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    pub(crate) fn dispatch(&mut self, msg: Msg) {
        for effect in self.state.update(msg) {
            match effect {
                Effect::Request { .. } | Effect::Notify(_) => self.backend.execute(effect),
                Effect::SetClipboard(text) => self.clipboard.set(&text),
                Effect::RequestPaste => {
                    if let Some(text) = self.clipboard.get() {
                        self.dispatch(Msg::Paste(text));
                    }
                }
                Effect::PickFolder => {
                    let picked = platform::pick_folder();
                    self.dispatch(Msg::FolderPicked(picked));
                }
                Effect::Reconnect(target) => self.backend.connect(target),
            }
        }
        self.request_redraw();
    }

    fn create_renderer(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        match SurfaceRenderer::new(window, size.width, size.height, self.scale()) {
            Ok(renderer) => {
                tracing::debug!("renderer ready");
                self.renderer = Some(renderer);
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot create renderer");
                self.state.error = Some(format!("graphics initialization failed: {e}"));
            }
        }
    }

    /// Where the UI goes, in logical pixels. On mobile that's the safe area (clear of the
    /// notch, status bar and home indicator). On desktop it's the whole window.
    fn content_rect(&self, window: &Window, scale: f32) -> Rect {
        if let Some([left, top, right, bottom]) = self.safe_area.as_ref().and_then(|f| f())
            && right > left
            && bottom > top
        {
            #[allow(clippy::cast_precision_loss, reason = "screen coordinates")]
            return Rect::new(
                left as f32 / scale,
                top as f32 / scale,
                (right - left) as f32 / scale,
                (bottom - top) as f32 / scale,
            );
        }
        let inner = window.inner_size();
        #[allow(
            clippy::cast_precision_loss,
            reason = "window sizes are far below 2^24"
        )]
        let (w, h) = (inner.width as f32 / scale, inner.height as f32 / scale);
        if cfg!(any(target_os = "ios", target_os = "android")) {
            let origin = window.inner_position().unwrap_or_default();
            #[allow(clippy::cast_precision_loss, reason = "safe-area offsets are small")]
            return Rect::new(origin.x as f32 / scale, origin.y as f32 / scale, w, h);
        }
        Rect::new(0.0, 0.0, w, h)
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let scale = self.scale();
        let size = window.inner_size();
        let bounds = self.content_rect(&window, scale);
        let compact = bounds.width < COMPACT_WIDTH;
        if compact != self.state.compact {
            self.state.update(Msg::SetCompact(compact));
        }
        tracing::debug!(
            width = size.width,
            height = size.height,
            scale,
            ?bounds,
            "redraw"
        );
        let root = view::view(&self.state, &self.theme, &self.scroll);
        let frame = build(&root, bounds, &mut self.text, self.pointer);
        for region in &frame.scroll_regions {
            self.scroll.insert(region.key, region.offset);
        }
        if let Some(renderer) = &mut self.renderer
            && let Err(e) = renderer.render(&frame.scene, &mut self.text, self.theme.background)
        {
            tracing::warn!(error = %e, "frame failed");
        }
        if let Some(access) = &mut self.access {
            let nodes = frame.access.clone();
            access.update_if_active(|| {
                urushi_ui::access::tree_update("Urushi", &nodes, None, f64::from(scale))
            });
        }
        let title = self
            .state
            .selected_thread
            .as_ref()
            .and_then(|t| self.state.workspace.thread(t))
            .map(|t| format!("{} — Urushi", t.title));
        window.set_title(title.as_deref().unwrap_or("Urushi"));
        let area = frame
            .hits
            .iter()
            .find(|h| h.key == Some(view::area_key()))
            .map(|h| h.rect);
        self.frame = Some(frame);
        if let Some(area) = area
            && self.state.area != Some(area)
        {
            self.dispatch(Msg::AreaChanged(area));
        }
    }

    fn update_cursor(&self, p: Point) {
        let (Some(window), Some(frame)) = (&self.window, &self.frame) else {
            return;
        };
        let mut icon = frame.cursor(p);
        if let (Some(area), Some(tab)) = (self.state.area, self.state.current_tab())
            && area.contains(p)
            && self.state.dialog.is_none()
            && let Some(tab) = self.state.workspace.tab(&tab.id)
            && let Some(divider) = crate::terminal::divider_at(tab.layout(), area, p)
        {
            icon = match divider.axis {
                urushi_core::Axis::Row => CursorIcon::ResizeColumn,
                urushi_core::Axis::Column => CursorIcon::ResizeRow,
            };
        }
        window.set_cursor(match icon {
            CursorIcon::Default => winit::window::CursorIcon::Default,
            CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
            CursorIcon::Text => winit::window::CursorIcon::Text,
            CursorIcon::ResizeColumn => winit::window::CursorIcon::ColResize,
            CursorIcon::ResizeRow => winit::window::CursorIcon::RowResize,
        });
    }

    fn mouse_button(&mut self, state: ElementState, button: MouseButton) {
        let Some(p) = self.pointer else { return };
        if button != MouseButton::Left {
            return;
        }
        match state {
            ElementState::Pressed => {
                let click = self.frame.as_ref().and_then(|f| f.click(p));
                if let Some(msg) = click {
                    self.dispatch(msg);
                } else if self.state.area.is_some_and(|a| a.contains(p))
                    && self.state.dialog.is_none()
                {
                    self.pressed_in_area = true;
                    self.dispatch(Msg::AreaPress(p));
                }
            }
            ElementState::Released => {
                if std::mem::take(&mut self.pressed_in_area) {
                    self.dispatch(Msg::AreaRelease(p));
                }
            }
        }
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        let Some(p) = self.pointer else { return };
        let (lines, pixels) = match delta {
            MouseScrollDelta::LineDelta(_, y) => (
                y * LINES_PER_NOTCH,
                y * LINES_PER_NOTCH * self.state.cell.height,
            ),
            #[allow(clippy::cast_possible_truncation, reason = "pixel deltas are small")]
            MouseScrollDelta::PixelDelta(d) => {
                let px = d.y as f32 / self.scale();
                (px / self.state.cell.height, px)
            }
        };
        if self.state.area.is_some_and(|a| a.contains(p)) && self.state.dialog.is_none() {
            self.wheel_remainder += lines;
            let whole = self.wheel_remainder.trunc();
            self.wheel_remainder -= whole;
            #[allow(clippy::cast_possible_truncation, reason = "bounded by wheel speed")]
            let whole = whole as i32;
            if whole != 0 {
                self.dispatch(Msg::AreaWheel(p, whole));
            }
        } else if let Some(region) = self
            .frame
            .as_ref()
            .and_then(|f| f.scroll_region(p))
            .copied()
        {
            let offset = (region.offset - pixels).clamp(0.0, region.max_offset());
            self.scroll.insert(region.key, offset);
            self.request_redraw();
        }
    }
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            // Mobile: the surface is recreated each time the app comes back.
            if self.renderer.is_none() {
                self.create_renderer();
                self.request_redraw();
            }
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title("Urushi")
            .with_visible(false);
        // Mobile windows are always full screen; a requested size would be taken literally.
        if !cfg!(any(target_os = "ios", target_os = "android")) {
            attributes = attributes
                .with_inner_size(LogicalSize::new(1280.0, 820.0))
                .with_min_inner_size(LogicalSize::new(640.0, 400.0));
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                tracing::error!(error = %e, "cannot create window");
                event_loop.exit();
                return;
            }
        };
        self.access = Some(Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        ));
        window.set_ime_allowed(cfg!(any(target_os = "ios", target_os = "android")));
        window.set_visible(true);
        self.window = Some(window);
        self.create_renderer();
        self.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.renderer = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(access), Some(window)) = (&mut self.access, &self.window) {
            access.process_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) => {
                let scale = self.scale();
                tracing::debug!(width = size.width, height = size.height, scale, "resized");
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height, scale);
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let scale = self.scale();
                if let (Some(renderer), Some(window)) = (&mut self.renderer, &self.window) {
                    let size = window.inner_size();
                    renderer.resize(size.width, size.height, scale);
                }
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                #[allow(clippy::cast_possible_truncation, reason = "screen coordinates")]
                let p = Point::new(
                    position.x as f32 / self.scale(),
                    position.y as f32 / self.scale(),
                );
                self.pointer = Some(p);
                if self.pressed_in_area {
                    self.dispatch(Msg::AreaDrag(p));
                }
                self.update_cursor(p);
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(state, button),
            // Touchscreens: a finger behaves like the left mouse button.
            WindowEvent::Touch(touch) => {
                #[allow(clippy::cast_possible_truncation, reason = "screen coordinates")]
                let p = Point::new(
                    touch.location.x as f32 / self.scale(),
                    touch.location.y as f32 / self.scale(),
                );
                self.pointer = Some(p);
                match touch.phase {
                    winit::event::TouchPhase::Started => {
                        self.mouse_button(ElementState::Pressed, MouseButton::Left);
                    }
                    winit::event::TouchPhase::Moved => {
                        if self.pressed_in_area {
                            self.dispatch(Msg::AreaDrag(p));
                        }
                    }
                    winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
                        self.mouse_button(ElementState::Released, MouseButton::Left);
                        self.pointer = None;
                    }
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::ModifiersChanged(m) => self.mods = Mods::from(m.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = KeyPress::from_winit(&event, self.mods) {
                    self.dispatch(Msg::Key(key));
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => self.dispatch(Msg::Text(text)),
            WindowEvent::DroppedFile(path) => {
                self.dispatch(Msg::OpenAddProject);
                self.dispatch(Msg::FolderPicked(Some(path.to_string_lossy().into_owned())));
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::App(msg) => self.dispatch(msg),
            UserEvent::Access(event) => match event.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => self.request_redraw(),
                accesskit_winit::WindowEvent::ActionRequested(request)
                    if request.action == accesskit::Action::Click =>
                {
                    let target = request.target_node.0;
                    let click = self.frame.as_ref().and_then(|f| {
                        let node = f.access.iter().find(|n| n.id.max(1) == target)?;
                        f.click(node.rect.center())
                    });
                    if let Some(msg) = click {
                        self.dispatch(msg);
                    }
                }
                _ => {}
            },
        }
    }
}
