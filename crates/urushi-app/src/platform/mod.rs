//! OS services the app uses: clipboard, folder and file pickers, and opening files with
//! the system's app (ADR 0007).
//! Desktop uses arboard/rfd. Mobile has stubs, since those UIs work differently.

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod desktop;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) use desktop::{
    Clipboard, HAS_FILE_PICKER, HAS_FOLDER_PICKER, HAS_SECONDARY_CLICK, open_path, pick_files,
    pick_folder, window_icon,
};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod mobile;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) use mobile::{
    Clipboard, HAS_FILE_PICKER, HAS_FOLDER_PICKER, HAS_SECONDARY_CLICK, open_path, pick_files,
    pick_folder, window_icon,
};
