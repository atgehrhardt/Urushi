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
use crate::msg::{Effect, Msg, SidebarMsg};
use crate::platform::{self, Clipboard};
use crate::state::AppState;
use crate::{UserEvent, view};

/// Window title; debug builds say so, so they're never mistaken for the real app.
const APP_TITLE: &str = if cfg!(debug_assertions) {
    "Urushi (dev)"
} else {
    "Urushi"
};

/// Lines scrolled per wheel notch.
const LINES_PER_NOTCH: f32 = 3.0;
/// Windows narrower than this (logical pixels) use the one-column phone layout.
const COMPACT_WIDTH: f32 = 700.0;
/// How far (logical pixels) a press must move before it becomes a drag.
const DRAG_THRESHOLD: f32 = 5.0;
/// Longest gap between clicks of a double or triple click.
const MULTI_CLICK: std::time::Duration = std::time::Duration::from_millis(450);
/// How far apart (logical pixels) clicks of a double or triple click may be.
const MULTI_CLICK_SLOP: f32 = 4.0;

/// A left press outside the terminal area that may turn into a drag.
pub(crate) struct Press {
    /// Where it started.
    at: Point,
    /// What to send if it becomes a drag (the source's `on_drag`).
    drag: Option<Msg>,
    /// Whether it already became one.
    dragging: bool,
}

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
    pub(crate) press: Option<Press>,
    pub(crate) wheel_remainder: f32,
    pub(crate) scroll: HashMap<Key, f32>,
    pub(crate) clipboard: Clipboard,
    pub(crate) safe_area: Option<crate::config::SafeAreaFn>,
    /// When the board panel's slide last advanced (`None`: not sliding).
    pub(crate) animated_at: Option<std::time::Instant>,
    /// The text field a press started in, while the button is held (drag selects).
    pub(crate) field_press: Option<Key>,
    /// The last left click: when, where, and how many quick clicks it made.
    pub(crate) last_click: Option<(std::time::Instant, Point, u8)>,
    /// Where the selection is remembered across launches, and what was last saved.
    pub(crate) ui_state: (Option<std::path::PathBuf>, crate::ui_state::UiState),
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
                Effect::PickFiles { task } => {
                    let paths = platform::pick_files();
                    self.read_files(&task, &paths);
                }
                Effect::ReadFiles { task, paths } => self.read_files(&task, &paths),
                Effect::OpenFile { id, name, bytes } => {
                    let opened = crate::files::save_for_opening(&id, &name, &bytes)
                        .and_then(|path| platform::open_path(&path));
                    if let Err(e) = opened {
                        let reason = format!("Can't open {name}: {e}");
                        self.dispatch(Msg::Board(crate::msg::BoardMsg::AttachFailed(reason)));
                    }
                }
                Effect::OpenUrl(url) => {
                    if let Err(e) = platform::open_url(&url) {
                        let reason = format!("Can't open {url}: {e}");
                        self.dispatch(Msg::Board(crate::msg::BoardMsg::AttachFailed(reason)));
                    }
                }
                Effect::SystemNotification { title, body } => platform::notify(&title, &body),
                Effect::StopDaemon => {
                    if let crate::config::ConnectTarget::Local(home) = &self.state.target {
                        self.backend.stop_daemon(home.clone());
                    }
                }
            }
        }
        self.remember_ui();
        self.request_redraw();
    }

    /// Reads files to attach to `task`, delivering each (or why it can't be attached).
    fn read_files(&mut self, task: &str, paths: &[String]) {
        use crate::msg::BoardMsg;
        for path in paths {
            let msg = match crate::files::read_attachment(path) {
                Ok((name, bytes)) => BoardMsg::Attach {
                    task: task.to_owned(),
                    name,
                    bytes,
                },
                Err(reason) => BoardMsg::AttachFailed(reason),
            };
            self.dispatch(Msg::Board(msg));
        }
    }

    /// Saves the selected thread and tabs, and the sidebar's width and collapse, when
    /// they change, for the next launch.
    fn remember_ui(&mut self) {
        // The file is the local daemon's; a window switched to a remote one keeps it.
        let (Some(path), saved) = &self.ui_state else {
            return;
        };
        if !matches!(self.state.target, crate::config::ConnectTarget::Local(_))
            // Written once when the drag ends, not on every move.
            || self.state.sidebar.resizing()
        {
            return;
        }
        let now = crate::ui_state::UiState::of(&self.state);
        if now != *saved {
            now.save(path);
            self.ui_state.1 = now;
        }
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

    /// Moves the board panel's and the sidebar's slides one frame towards where they're
    /// heading, and keeps redrawing until both get there.
    fn animate(&mut self) {
        let board = (
            self.state.board.reveal,
            if self.state.board.open { 1.0 } else { 0.0 },
        );
        let sidebar = (
            self.state.sidebar.reveal(),
            self.state.sidebar.reveal_target(),
        );
        let moving = |(current, target): (f32, f32)| (current - target).abs() >= f32::EPSILON;
        if !moving(board) && !moving(sidebar) {
            self.animated_at = None;
            return;
        }
        let now = std::time::Instant::now();
        // The first frame of a slide moves one 60 Hz step; later ones by elapsed time.
        let elapsed = self
            .animated_at
            .map_or(1.0 / 60.0, |at| now.duration_since(at).as_secs_f32())
            .min(0.05);
        self.animated_at = Some(now);
        let advance = |(current, target): (f32, f32), seconds: f32| {
            let step = elapsed / seconds;
            if target > current {
                (current + step).min(target)
            } else {
                (current - step).max(target)
            }
        };
        if moving(board) {
            let next = advance(board, crate::board::SLIDE_SECONDS);
            self.state.update(Msg::BoardReveal(next));
        }
        if moving(sidebar) {
            let next = advance(sidebar, crate::sidebar::SLIDE_SECONDS);
            self.state.update(Msg::Sidebar(SidebarMsg::Reveal(next)));
        }
        self.request_redraw();
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        self.animate();
        let scale = self.scale();
        let size = window.inner_size();
        let bounds = self.content_rect(&window, scale);
        let compact = bounds.width < COMPACT_WIDTH;
        if compact != self.state.compact {
            self.state.update(Msg::SetCompact(compact));
        }
        if (bounds.width - self.state.sidebar.window_width()).abs() >= 0.5 {
            self.dispatch(Msg::Sidebar(SidebarMsg::WindowWidth(bounds.width)));
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
            .map(|t| format!("{} — {APP_TITLE}", t.title));
        window.set_title(title.as_deref().unwrap_or(APP_TITLE));
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
        if self.state.sidebar.resizing() {
            // Keep the resize cursor even where the pointer has outrun the edge.
            icon = CursorIcon::ResizeColumn;
        } else if let (Some(area), Some(tab)) = (self.state.area, self.state.current_tab())
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
        let in_area =
            self.state.area.is_some_and(|a| a.contains(p)) && !self.state.terminals_covered();
        // A secondary click is a right click, or a two-finger click on a trackpad
        // (macOS reports both as the right button). Over a terminal it opens the pane's
        // menu, and the middle button goes to the app (`update/pointer.rs`).
        if matches!(button, MouseButton::Right | MouseButton::Middle) {
            let pressed = state == ElementState::Pressed;
            let menu = (button == MouseButton::Right && pressed)
                .then(|| self.frame.as_ref().and_then(|f| f.secondary_click(p)))
                .flatten();
            if let Some(msg) = menu {
                self.dispatch(Msg::Pointer(p));
                self.dispatch(msg);
            } else if in_area {
                let button = if button == MouseButton::Right {
                    urushi_client::v1::MouseButton::Right
                } else {
                    urushi_client::v1::MouseButton::Middle
                };
                self.dispatch(Msg::AreaButton {
                    at: p,
                    button,
                    pressed,
                });
            }
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        match state {
            ElementState::Pressed => {
                self.dispatch(Msg::Pointer(p));
                let clicks = self.count_click(p);
                let (click, drag, field) = self.frame.as_ref().map_or((None, None, None), |f| {
                    (f.click(p), f.drag_source(p), f.field_at(p).cloned())
                });
                if let Some(msg) = click {
                    self.press = Some(Press {
                        at: p,
                        drag,
                        dragging: false,
                    });
                    self.dispatch(msg);
                    // A press in a text field (now focused) places the caret there.
                    if let Some(field) = field {
                        let caret = urushi_ui::caret_at(&mut self.text, &field, p);
                        self.field_press = field.key;
                        self.dispatch(Msg::FieldPress {
                            caret,
                            extend: self.mods.shift,
                            clicks,
                        });
                    }
                } else if self.state.area.is_some_and(|a| a.contains(p))
                    && !self.state.terminals_covered()
                {
                    self.pressed_in_area = true;
                    self.dispatch(Msg::AreaPress(p));
                }
            }
            ElementState::Released => {
                self.field_press = None;
                if std::mem::take(&mut self.pressed_in_area) {
                    self.dispatch(Msg::AreaRelease(p));
                }
                if let Some(press) = self.press.take() {
                    if press.dragging {
                        let drop = self.frame.as_ref().and_then(|f| f.drop_target(p));
                        self.dispatch(drop.unwrap_or(Msg::DragCancel));
                    } else {
                        // A click that didn't become a drag (opens a pressed task card).
                        self.dispatch(Msg::PressEnded);
                    }
                }
            }
        }
    }

    /// How many quick clicks in one spot this press makes (1, 2 or 3; a fourth starts
    /// over).
    fn count_click(&mut self, p: Point) -> u8 {
        let now = std::time::Instant::now();
        let clicks = match self.last_click {
            Some((at, where_, n))
                if now.duration_since(at) <= MULTI_CLICK
                    && (p.x - where_.x).hypot(p.y - where_.y) <= MULTI_CLICK_SLOP =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, p, clicks));
        clicks
    }

    /// The pointer moved with the left button down outside the terminal area: select
    /// text in a field, or start or continue a drag.
    fn drag_to(&mut self, p: Point) {
        if let Some(key) = self.field_press {
            let field = self.frame.as_ref().and_then(|f| f.field(key)).cloned();
            if let Some(field) = field {
                let caret = urushi_ui::caret_at(&mut self.text, &field, p);
                self.dispatch(Msg::FieldDrag(caret));
            }
            return;
        }
        let Some(press) = &mut self.press else { return };
        if press.dragging {
            self.dispatch(Msg::DragMove(p));
            self.drag_over(p);
            return;
        }
        let moved = (p.x - press.at.x).hypot(p.y - press.at.y);
        if moved >= DRAG_THRESHOLD
            && let Some(start) = press.drag.take()
        {
            press.dragging = true;
            self.dispatch(start);
            self.dispatch(Msg::DragMove(p));
            self.drag_over(p);
        }
    }

    /// Tells the app what a drag is over, so it can show where a drop would land.
    fn drag_over(&mut self, p: Point) {
        let target = self.frame.as_ref().and_then(|f| f.drop_target(p));
        self.dispatch(Msg::DragOver(target.map(Box::new)));
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
        if self.state.area.is_some_and(|a| a.contains(p)) && !self.state.terminals_covered() {
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
            .with_title(APP_TITLE)
            .with_window_icon(platform::window_icon())
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
            WindowEvent::Focused(focused) => self.dispatch(Msg::WindowFocused(focused)),
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
                } else if self.press.is_none()
                    && !self.state.terminals_covered()
                    && self.state.area.is_some_and(|a| a.contains(p))
                {
                    self.dispatch(Msg::AreaHover(p));
                }
                self.drag_to(p);
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
                        self.drag_to(p);
                    }
                    winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
                        self.mouse_button(ElementState::Released, MouseButton::Left);
                        self.pointer = None;
                    }
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::ModifiersChanged(m) => {
                self.mods = Mods::from(m.state());
                self.dispatch(Msg::Modifiers(self.mods));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = KeyPress::from_winit(&event, self.mods) {
                    self.dispatch(Msg::Key(key));
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => self.dispatch(Msg::Text(text)),
            WindowEvent::DroppedFile(path) => {
                // The pointer may be stale during a drag; `update` decides what to do.
                let over_terminal = self
                    .pointer
                    .zip(self.state.area)
                    .is_some_and(|(p, area)| area.contains(p));
                self.dispatch(Msg::FileDropped {
                    is_dir: path.is_dir(),
                    path: path.to_string_lossy().into_owned(),
                    over_terminal,
                });
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::App(msg) => self.dispatch(*msg),
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
