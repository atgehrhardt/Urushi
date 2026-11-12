//! The Urushi graphical app: a workspace that *contains* terminals.
//!
//! Elm architecture (ADR 0006):
//! - [`state::AppState`] is all application state.
//! - `update.rs` changes state in response to [`msg::Msg`]s and returns [`msg::Effect`]s.
//!   It is pure and unit-tested.
//! - `view/` turns state into a `urushi-ui` node tree.
//! - `runner.rs` (winit) feeds OS events in, draws frames with `urushi-render`, and hands
//!   effects to `backend.rs`, which talks to the daemon on a background runtime.
//!
//! This crate is platform-neutral. `apps/desktop`, `apps/ios` and `apps/android` are thin
//! entry points that call [`run`] or [`run_with`].

pub mod appearance;
pub mod attention;
mod backend;
pub mod board;
mod brand_icons;
pub mod config;
pub mod creations;
pub mod deletions;
pub mod divider;
pub mod drafts;
pub mod errors;
mod files;
pub mod filter;
pub mod issue;
pub mod keys;
pub mod msg;
pub mod organize;
mod platform;
pub mod project_settings;
mod pulse;
pub mod quick_terminal;
mod runner;
pub mod running;
mod scrolling;
pub mod sidebar;
pub mod snapshot;
mod soft_keyboard;
pub mod state;
pub mod surface;
pub mod task_agent;
pub mod terminal;
mod touch;
pub mod ui_state;
mod update;
pub mod updates;
mod view;

use std::collections::HashMap;

use thiserror::Error;
use urushi_ui::{TextSystem, Theme};
use winit::event_loop::EventLoop;

pub use config::{AppConfig, ConnectTarget, SafeAreaFn, SoftKeyboardFn};

/// Events delivered to the winit loop from other threads.
#[derive(Debug)]
pub enum UserEvent {
    /// A message for the app (from the backend). Boxed: messages are large and rare
    /// compared to accessibility events.
    App(Box<msg::Msg>),
    /// An accessibility request.
    Access(accesskit_winit::Event),
    /// Quit the app (it relaunches itself after an update).
    Quit,
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

/// App failures that stop it from running.
#[derive(Debug, Error)]
pub enum AppError {
    /// The windowing system failed.
    #[error("windowing error: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// The background runtime couldn't start.
    #[error("cannot start the network runtime: {0}")]
    Runtime(#[from] std::io::Error),
}

/// Creates the event loop and runs the app until its window closes.
pub fn run(config: AppConfig) -> Result<(), AppError> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    run_with(event_loop, config)
}

/// Runs the app on an event loop the platform shell created (e.g. Android's, which needs
/// the activity handle).
pub fn run_with(event_loop: EventLoop<UserEvent>, config: AppConfig) -> Result<(), AppError> {
    let proxy = event_loop.create_proxy();
    let theme = Theme::dark();
    tracing::debug!("loading fonts");
    let mut text = TextSystem::new();
    tracing::debug!("fonts loaded");
    let cell = text.cell_metrics(theme.terminal_font_size);
    let keyring_dir = config.data_dir.clone().or_else(urushi_client::home_dir);
    // With no host chosen (phones), go back to the one used last, if any (ADR 0043).
    let remembered = (config.target == ConnectTarget::Ask)
        .then(|| keyring_dir.as_deref().and_then(last_host))
        .flatten();
    let target = remembered
        .clone()
        .map_or(config.target, ConnectTarget::Remote);
    let backend = backend::Backend::start(&target, proxy.clone(), keyring_dir)?;
    // Show the thread and tabs the window showed when it was last closed.
    let ui_state_path = ui_state::UiState::path(&target);
    let ui_state = ui_state_path
        .as_deref()
        .map(ui_state::UiState::load)
        .unwrap_or_default();
    let mut state = match remembered {
        Some(host) => state::AppState::resuming(host, cell),
        None => state::AppState::new(target, cell),
    };
    ui_state.clone().apply(&mut state);
    let mut runner = runner::Runner {
        state,
        ui_state: (ui_state_path, ui_state),
        theme,
        look: urushi_core::Appearance::default(),
        text,
        backend,
        proxy,
        window: None,
        renderer: None,
        access: None,
        frame: None,
        pointer: None,
        mods: keys::Mods::default(),
        pressed_in_area: false,
        press: None,
        wheel_remainder: 0.0,
        touches: touch::Touches::default(),
        scroll: HashMap::new(),
        clipboard: platform::Clipboard::new(),
        safe_area: config.safe_area.clone(),
        last_safe_area: None,
        keyboard: soft_keyboard::SoftKeyboard::default(),
        soft_keyboard: config.soft_keyboard.clone(),
        animated_at: None,
        pulse: pulse::Pulse::new(std::time::Instant::now()),
        occluded: false,
        field_press: None,
        last_click: None,
    };
    event_loop.run_app(&mut runner)?;
    Ok(())
}

/// The remote host this device connected to last, from its recent hosts in `dir`.
fn last_host(dir: &std::path::Path) -> Option<String> {
    match urushi_client::RecentHosts::open(dir) {
        Ok(recent) => recent.last().map(str::to_owned),
        Err(e) => {
            tracing::warn!(error = %e, "cannot read the recent hosts");
            None
        }
    }
}

/// Sends panics to the log (`tracing`) with where they happened and a backtrace, before
/// the default handler runs. Shells whose stderr goes nowhere (Android, an iOS device)
/// call it after setting up logging, so a crash leaves a reason in logcat or the device
/// log.
pub fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("panic: {info}\n{backtrace}");
        default(info);
    }));
}
