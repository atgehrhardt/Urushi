//! OS services the app uses: clipboard and folder picker (ADR 0007).
//! Desktop uses arboard/rfd. Mobile has stubs, since those UIs work differently.

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod desktop;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) use desktop::{Clipboard, pick_folder};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod mobile;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) use mobile::{Clipboard, pick_folder};
