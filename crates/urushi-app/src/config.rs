//! How the app finds its daemon, plus hooks a platform shell can provide.

use std::path::PathBuf;
use std::sync::Arc;

/// Which daemon to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectTarget {
    /// The local daemon, started on demand. `None` home uses the default.
    Local(Option<PathBuf>),
    /// A remote daemon at `host[:port]` (usually a tailnet name).
    Remote(String),
    /// Not chosen yet (mobile, first launch): show the connect dialog.
    Ask,
}

/// Returns the area free of system bars and notches as `[left, top, right, bottom]` in
/// physical pixels, for platforms where winit can't report it (Android).
pub type SafeAreaFn = Arc<dyn Fn() -> Option<[i32; 4]> + Send + Sync>;

/// App startup options.
#[derive(Clone)]
pub struct AppConfig {
    /// Which daemon to use at startup.
    pub target: ConnectTarget,
    /// Platform-provided safe area, if winit's isn't accurate.
    pub safe_area: Option<SafeAreaFn>,
}

impl AppConfig {
    /// Options for connecting to `target`, with no platform hooks.
    pub fn new(target: ConnectTarget) -> Self {
        Self {
            target,
            safe_area: None,
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
            .finish()
    }
}
