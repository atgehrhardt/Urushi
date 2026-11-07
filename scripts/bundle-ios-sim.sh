#!/usr/bin/env bash
# Builds build/ios-sim/Urushi.app for the iOS Simulator. With `--run`, also boots a
# simulator (headless), installs the app, launches it, and saves a screenshot.
# Device builds use target aarch64-apple-ios plus a signing identity (see docs/platforms.md).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --target aarch64-apple-ios-sim -p urushi-ios
APP="build/ios-sim/Urushi.app"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
rm -rf "$APP" && mkdir -p "$APP"
cp target/aarch64-apple-ios-sim/debug/urushi-ios "$APP/Urushi"
cat > "$APP/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Urushi</string>
  <key>CFBundleIdentifier</key><string>app.urushi.Urushi</string>
  <key>CFBundleExecutable</key><string>Urushi</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>MinimumOSVersion</key><string>16.0</string>
  <key>UILaunchScreen</key><dict/>
  <key>UIRequiresFullScreen</key><true/>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>NSLocalNetworkUsageDescription</key><string>Urushi connects to your Urushi host over your tailnet.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "built $APP"
if [[ "${1:-}" == "--run" ]]; then
  DEVICE="${IOS_DEVICE:-iPhone 16 Pro}"
  xcrun simctl boot "$DEVICE" 2>/dev/null || true
  xcrun simctl install "$DEVICE" "$APP"
  xcrun simctl launch "$DEVICE" app.urushi.Urushi
  sleep 4
  xcrun simctl io "$DEVICE" screenshot build/ios-sim/screenshot.png
  echo "screenshot: build/ios-sim/screenshot.png"
fi
