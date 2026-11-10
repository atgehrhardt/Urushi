# Platforms and builds

| Platform | Role | Build | Verified |
|---|---|---|---|
| macOS | daemon + app + CLI | `make app` → `build/Urushi.app` (`Urushi` and `urushid` in `Contents/MacOS`, the CLI in `Contents/Resources/bin/urushi`). Pushing a `vX.Y.Z` tag publishes it as a GitHub Release ([CI and releases](features/ci-and-releases.md)). | Yes: GUI, input, accessibility, dialogs, tailnet serving |
| Linux | daemon + app + CLI | `cargo build --release -p urushi-desktop -p urushi-daemon -p urushi-cli` | Type-checks (`cargo check --target x86_64-unknown-linux-gnu`); CI runs the tests |
| Windows | daemon + app + CLI | Same as Linux. The local daemon listens on a per-home named pipe. | Type-checks (`--target x86_64-pc-windows-msvc`); CI runs the tests |
| iOS | remote client | `make ios-sim` (simulator .app, installs, launches and screenshots). Device builds use `aarch64-apple-ios` plus signing. | Yes, in the simulator: connected over the tailnet and rendered the workspace |
| Android | remote client | `make android` → `build/android/urushi.apk` (NDK and SDK command-line tools, no Gradle) | Yes, in the emulator: connected over the tailnet, opened split terminals, typed into a shell on the Mac |

## Installing on macOS for daily use
- `make install` builds the release bundle and swaps it into `/Applications/Urushi.app` (override with `INSTALL_DIR`). The swap uses renames, so a running app and daemon are never disturbed.
- The app starts `urushid` from its own bundle when none is running. The daemon then keeps running when the app quits, so your sessions survive app restarts and upgrades.
- A new daemon version takes effect only when the daemon next starts (e.g. after a reboot). Restarting the daemon ends its terminal sessions, so Urushi never does it for you.
- **Developing Urushi inside Urushi:**
  - Debug builds default to a separate dev home (`~/Library/Application Support/Urushi Dev`). `scripts/dev.sh` wraps the loop: `run` (dev app plus a fresh dev daemon), `cli`, `test`, `status`, `stop`, and `ship` (tests, then install).
  - The dev app is titled "Urushi (dev)" and shows a DEV badge.

## Mobile clients
- iOS and Android can't host agent shells, so they never start a local daemon.
- **Connecting:**
  - They open with **Connect to a host**. Enter the MagicDNS name or tailnet IP of a machine where `urushi tailnet on` (or Settings → Serve on my tailnet) is enabled.
  - On iOS, setting `URUSHI_HOST` skips the dialog (e.g. `SIMCTL_CHILD_URUSHI_HOST=host:port`).
- **Layout and input:**
  - Windows narrower than 700 logical pixels use the one-column layout: the thread list, then a thread's terminals with a back button.
  - A key bar adds Esc, Tab, ^C, ^D and arrows.
  - Touch acts as the left mouse button.
- **Platform hooks:**
  - Android: the shell passes `android-activity`'s content rect as `AppConfig::safe_area`, because winit doesn't report system-bar insets there.
  - iOS: the app lays out inside winit's safe area.
  - Android: wgpu runs without debug labels, because some Vulkan drivers (including the emulator's) crash on them.
  - Android: logs go to logcat under the tag `urushi`.

## App icon
- **Source:** `assets/icon/icon.svg` (the app tile), `mark-dark.svg` and `mark-light.svg` (the mark alone, for dark and light backgrounds). They are the "Leaf tag" design: code brackets around a lacquer-tree (*Toxicodendron vernicifluum*) sprig, on lacquer black `#15110F` with urushi red `#C8432F`.
- **Rendered files are committed**, so builds need no tools. Re-render with `make icons` (`scripts/render-icons.swift`, macOS only) after changing the SVG:

  | Platform | File | How it's laid out |
  |---|---|---|
  | macOS | `assets/icon/Urushi.icns` | Tile at 824 of 1024 (Apple's icon grid); `bundle-macos.sh` sets `CFBundleIconFile`. |
  | iOS | `apps/ios/icons/AppIcon*.png` | Full bleed (iOS masks the corners); `CFBundleIcons` in `bundle-ios-sim.sh`. |
  | Android | `apps/android/res/` | Adaptive icon: mark in the 72 dp safe area on a `#15110F` background; compiled by `aapt2` in `build-android.sh`. |
  | Windows, Linux | `assets/icon/window-64.rgba` | Raw RGBA embedded as the window and taskbar icon (`platform::window_icon`). |
  | In the app (all) | `assets/icon/mark-128.rgba` | The mark alone on transparent, raw 128×128 RGBA: the logo beside the name in the sidebar header (`view/sidebar.rs::LOGO`). |

## Rendering without a window
`cargo run -p urushi-app --example snapshot -- out.bmp <width> <height> <scale> [connect]` renders the UI offscreen. Use it to check layouts at phone and desktop sizes.

## Known gaps
- **Soft keyboard:** the Android soft keyboard doesn't open for `NativeActivity` (a winit limitation). Hardware keyboards and the key bar work. A small Java `showSoftInput` shim is the follow-up.
- **Edge-to-edge:** Android targets SDK 34 to avoid forced edge-to-edge until insets are handled everywhere.
- **Signing:** releases are ad-hoc signed and not notarized ([CI and releases](features/ci-and-releases.md), ADR 0032), so macOS asks the user to allow the first launch. Windows and Linux have no release builds yet.
- **Icons:** there's no Windows `.ico` embedded in the `.exe` yet (the window icon is set at runtime), and the iOS icons are loose files rather than an asset catalog.
