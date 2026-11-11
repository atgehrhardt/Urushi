# Platforms and builds

| Platform | Role | Build | Verified |
|---|---|---|---|
| macOS | daemon + app + CLI | `make app` → `build/Urushi.app` (`Urushi` and `urushid` in `Contents/MacOS`, the CLI in `Contents/Resources/bin/urushi`). Pushing a `vX.Y.Z` tag publishes it as a GitHub Release ([CI and releases](features/ci-and-releases.md)). | Yes: GUI, input, accessibility, dialogs, tailnet serving |
| Linux | daemon + app + CLI | `cargo build --release -p urushi-desktop -p urushi-daemon -p urushi-cli` | Type-checks (`cargo check --target x86_64-unknown-linux-gnu`); CI runs the tests |
| Windows | daemon + app + CLI | Same as Linux. The local daemon listens on a per-home named pipe. | Type-checks (`--target x86_64-pc-windows-msvc`); CI runs the unit tests and the in-process daemon tests that don't need a Unix shell (e.g. the Jira ones) |
| iOS | remote client | `make ios-sim` (simulator .app, installs, launches and screenshots). `make ios` → `build/ios/Urushi.ipa`, an **unsigned** device build. Both come from `scripts/bundle-ios.sh`, and CI builds both on every pull request. Not released (needs a paid Apple Developer account, ADR 0040). | Yes, in the simulator (iOS 18.3, 2026-11-10): connected to a daemon, rendered the workspace and the first-run setup dialog |
| Android | remote client | `make android` → `build/android/urushi.apk` (NDK and SDK command-line tools, no Gradle; macOS or Linux). CI builds it on every pull request, and tagged releases publish it ([CI and releases](features/ci-and-releases.md)). | Yes, in the emulator (API 36, arm64, 2026-11-10), headless and with a window (host GPU): connected through the dialog, opened a thread, typed into a shell on the Mac, ^C from the key bar, split a pane |

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
  - Touch (`touch.rs`, unit-tested):
    - Dragging a finger scrolls whatever is under it. In a terminal it works like the wheel: scrollback, or the running app's own scrolling.
    - A tap clicks.
    - Holding still for 0.4 s, then moving, acts as the left button: select text, reorder threads.
    - A touch that starts on a split divider drags it at once.
    - Only one finger is followed. There's no fling momentum yet.
- **Platform hooks:**
  - Android: the shell passes `android-activity`'s content rect as `AppConfig::safe_area`, because winit doesn't report system-bar insets there.
  - iOS: the app lays out inside winit's safe area.
  - Android: wgpu runs without debug labels, because some Vulkan drivers (including the emulator's) crash on them.
  - Android: the renderer tries Vulkan, then GL, with one backend per wgpu instance (`urushi_render::surface::backend_attempts`). With both in one instance, the emulator picked GL and its EGL surface failed on the already-connected window, which crashed the app at start.
  - Android: logs go to logcat under the tag `urushi`.

## Installing the unsigned iOS build on a device
`Urushi.ipa` (from `make ios`, or CI's `urushi-ios-unsigned` artifact) isn't signed, so an
iPhone won't install it as is. Re-sign it with your own Apple ID, using a sideloading tool
such as Sideloadly or AltStore, or with Xcode. With a free Apple ID, the signature expires
after 7 days and the app has to be re-signed. A paid developer account removes that limit
and enables TestFlight.

## Testing the mobile apps on this machine
The apps only talk to a daemon that serves "on the tailnet", which means the addresses
`tailscale status` reports. To test against a throwaway daemon without Tailscale, put a
fake `tailscale` first on `PATH` that reports loopback. `CliTailscale` searches `PATH`
before the standard install locations:

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # rustup's cargo, not Homebrew's (it lacks the mobile targets)
dir=$(mktemp -d); mkdir "$dir/bin"
cat > "$dir/bin/tailscale" <<'EOF'
#!/bin/sh
case "$1" in
  status) echo '{"BackendState":"Running","Self":{"HostName":"mac","DNSName":"mac.test.ts.net.","TailscaleIPs":["127.0.0.1"],"UserID":1,"Online":true,"OS":"macOS"},"User":{"1":{"LoginName":"tester@example.com"}}}' ;;
  whois) echo '{"Node":{"Name":"emulator.","Tags":null},"UserProfile":{"LoginName":"tester@example.com"}}' ;;
esac
EOF
chmod +x "$dir/bin/tailscale"
export PATH="$dir/bin:$PATH" URUSHI_HOME="$dir/home"
cargo build -p urushi-daemon -p urushi-cli && U=./target/debug/urushi
$U settings set tailnet-port 7431 && $U settings set task-agent claude && $U tailnet on
$U repo add <some git repo> && $U thread new <repo-id> "Mobile test"
```

- **iOS:** the simulator shares the Mac's network. Run `./scripts/bundle-ios.sh`, then
  `SIMCTL_CHILD_URUSHI_HOST=127.0.0.1:7431 xcrun simctl launch "iPhone 16 Pro" app.urushi.Urushi`.
- **Android:** the emulator reaches the Mac's loopback at `10.0.2.2`. Run
  `make android && adb install -r build/android/urushi.apk`, launch it, then type the host with
  `adb shell input text 10.0.2.2:7431 && adb shell input keyevent 66`. Drive it with
  `adb shell input tap X Y`, take screenshots with `adb exec-out screencap -p`, and read the
  daemon's side with `$U screen <pane-id>`.
- **Clean up:** `$U daemon stop --yes` with the same `URUSHI_HOME` still set. It stops only that
  daemon. Never stop a daemon by name.

Setting a task agent skips the first-run setup dialog. Never use the default port 7420:
the real daemon may be serving on it.

## App icon
- **Source:** `assets/icon/icon.svg` (the app tile), `mark-dark.svg` and `mark-light.svg` (the mark alone, for dark and light backgrounds). They are the "Leaf tag" design: code brackets around a lacquer-tree (*Toxicodendron vernicifluum*) sprig, on lacquer black `#15110F` with urushi red `#C8432F`.
- **Rendered files are committed**, so builds need no tools. Re-render with `make icons` (`scripts/render-icons.swift`, macOS only) after changing the SVG:

  | Platform | File | How it's laid out |
  |---|---|---|
  | macOS | `assets/icon/Urushi.icns` | Tile at 824 of 1024 (Apple's icon grid); `bundle-macos.sh` sets `CFBundleIconFile`. |
  | iOS | `apps/ios/icons/AppIcon*.png` | Full bleed (iOS masks the corners); `CFBundleIcons` in `bundle-ios.sh`. |
  | Android | `apps/android/res/` | Adaptive icon: mark in the 72 dp safe area on a `#15110F` background; compiled by `aapt2` in `build-android.sh`. |
  | Windows, Linux | `assets/icon/window-64.rgba` | Raw RGBA embedded as the window and taskbar icon (`platform::window_icon`). |
  | In the app (all) | `assets/icon/mark-128.rgba` | The mark alone on transparent, raw 128×128 RGBA: the logo beside the name in the sidebar header (`view/sidebar.rs::LOGO`). |

## Rendering without a window
`cargo run -p urushi-app --example snapshot -- out.bmp <width> <height> <scale> [connect]` renders the UI offscreen. Use it to check layouts at phone and desktop sizes.

## Known gaps
- **Host isn't remembered:** the mobile apps open the connect dialog on every launch.
  There's no saved host yet. On iOS, `URUSHI_HOST` is the only way to skip it.
- **Soft keyboard:** the Android soft keyboard doesn't open for `NativeActivity` (a winit limitation). Hardware keyboards and the key bar work. A small Java `showSoftInput` shim is the follow-up.
- **Edge-to-edge:** Android targets SDK 34 to avoid forced edge-to-edge until insets are handled everywhere.
- **Signing:** releases are ad-hoc signed and not notarized ([CI and releases](features/ci-and-releases.md), ADR 0032), so macOS asks the user to allow the first launch. Windows and Linux have no release builds yet.
- **Icons:** there's no Windows `.ico` embedded in the `.exe` yet (the window icon is set at runtime), and the iOS icons are loose files rather than an asset catalog.
