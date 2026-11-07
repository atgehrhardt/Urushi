#!/usr/bin/env bash
# Builds build/Urushi.app: the GUI plus urushid (started on demand, next to the app
# binary) and the urushi CLI.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${PROFILE:-release}"
cargo build --profile "$PROFILE" -p urushi-desktop -p urushi-daemon -p urushi-cli
TARGET_DIR="target/$PROFILE"
APP="build/Urushi.app"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET_DIR/urushi-app" "$APP/Contents/MacOS/Urushi"
cp "$TARGET_DIR/urushid" "$APP/Contents/MacOS/urushid"
cp "$TARGET_DIR/urushi" "$APP/Contents/MacOS/urushi"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Urushi</string>
  <key>CFBundleDisplayName</key><string>Urushi</string>
  <key>CFBundleIdentifier</key><string>app.urushi.Urushi</string>
  <key>CFBundleExecutable</key><string>Urushi</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST
# Ad-hoc signature so Gatekeeper on Apple Silicon will launch it locally.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
echo "built $APP"
