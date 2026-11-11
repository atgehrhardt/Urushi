//! How the app finds its daemon, plus hooks a platform shell can provide.

use std::path::PathBuf;
use std::sync::Arc;

/// Which daemon to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectTarget {
    /// The local daemon, started on demand. `None` home uses the default.
    Local(Option<PathBuf>),
    /// A remote daemon at `host[:port]`: a tailnet name, or a LAN host this device
    /// paired with (reached through the encrypted LAN channel).
    Remote(String),
    /// Not chosen yet (mobile): go back to the remote host connected to last, if this
    /// device remembers one (ADR 0043), else show the connect dialog.
    Ask,
}

/// Returns the area free of system bars and notches as `[left, top, right, bottom]` in
/// physical pixels, for platforms where winit can't report it (Android).
pub type SafeAreaFn = Arc<dyn Fn() -> Option<[i32; 4]> + Send + Sync>;

/// Shows (`true`) or hides (`false`) the on-screen keyboard, for platforms where winit's
/// `set_ime_allowed` can't (Android's `NativeActivity`).
pub type SoftKeyboardFn = Arc<dyn Fn(bool) + Send + Sync>;

/// App startup options.
#[derive(Clone)]
pub struct AppConfig {
    /// Which daemon to use at startup.
    pub target: ConnectTarget,
    /// Platform-provided safe area, if winit's isn't accurate.
    pub safe_area: Option<SafeAreaFn>,
    /// Platform-provided on-screen keyboard switch, if winit's doesn't work.
    pub soft_keyboard: Option<SoftKeyboardFn>,
    /// Where this device keeps its LAN keyring (ADR 0041). `None`: the local home
    /// (`URUSHI_HOME`, then the app data dir). Mobile shells pass their app's data dir.
    pub data_dir: Option<PathBuf>,
}

impl AppConfig {
    /// Options for connecting to `target`, with no platform hooks.
    pub fn new(target: ConnectTarget) -> Self {
        Self {
            target,
            safe_area: None,
            soft_keyboard: None,
            data_dir: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new(ConnectTarget::Local(None))
    }
}

impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("target", &self.target)
            .field("safe_area", &self.safe_area.is_some())
            .field("soft_keyboard", &self.soft_keyboard.is_some())
            .field("data_dir", &self.data_dir)
            .finish()
    }
}
