#!/usr/bin/env bash
# Builds the iOS app without Xcode projects.
#
#   bundle-ios.sh [--run]   build/ios-sim/Urushi.app for the iOS Simulator (debug, ad-hoc
#                           signed). With --run, also boots a simulator (headless),
#                           installs the app, launches it, and saves a screenshot.
#   bundle-ios.sh --device  build/ios/Urushi.ipa for iPhones and iPads (release, arm64),
#                           UNSIGNED. A device only runs it after it is re-signed with an
#                           Apple ID or a developer certificate (Xcode, Sideloadly,
#                           AltStore); see docs/platforms.md.
set -euo pipefail
cd "$(dirname "$0")/.."
MODE=sim RUN=false
case "${1:-}" in
  --device) MODE=device ;;
  --run) RUN=true ;;
  "") ;;
  *) echo "usage: $0 [--run | --device]" >&2; exit 2 ;;
esac
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

if [[ "$MODE" == device ]]; then
  TARGET=aarch64-apple-ios PROFILE_DIR=release PLATFORM=iPhoneOS OUT=build/ios
  cargo build --release --target "$TARGET" -p urushi-ios
else
  TARGET=aarch64-apple-ios-sim PROFILE_DIR=debug PLATFORM=iPhoneSimulator OUT=build/ios-sim
  cargo build --target "$TARGET" -p urushi-ios
fi
APP="$OUT/Urushi.app"
rm -rf "$OUT" && mkdir -p "$APP"
# Keep build outputs out of Spotlight/Launchpad (only /Applications/Urushi.app should show).
mkdir -p build && touch build/.metadata_never_index
cp "target/$TARGET/$PROFILE_DIR/urushi-ios" "$APP/Urushi"
# Home-screen icons (rendered by scripts/render-icons.swift).
cp apps/ios/icons/*.png "$APP/"
cat > "$APP/Info.plist" <<PLIST
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
  <key>CFBundleSupportedPlatforms</key><array><string>${PLATFORM}</string></array>
  <key>LSRequiresIPhoneOS</key><true/>
  <key>MinimumOSVersion</key><string>16.0</string>
  <key>UIRequiredDeviceCapabilities</key><array><string>arm64</string></array>
  <key>UILaunchScreen</key><dict/>
  <key>CFBundleIcons</key><dict><key>CFBundlePrimaryIcon</key><dict>
    <key>CFBundleIconFiles</key><array><string>AppIcon60x60</string></array>
  </dict></dict>
  <key>CFBundleIcons~ipad</key><dict><key>CFBundlePrimaryIcon</key><dict>
    <key>CFBundleIconFiles</key><array><string>AppIcon60x60</string><string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array>
  </dict></dict>
  <key>UIRequiresFullScreen</key><true/>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>NSLocalNetworkUsageDescription</key><string>Urushi connects to your Urushi host over your tailnet or your local network.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Info.plist" >/dev/null

if [[ "$MODE" == device ]]; then
  # An .ipa is a zip of Payload/<name>.app. Left unsigned: an ad-hoc signature doesn't
  # run on devices, and re-signing tools replace any signature anyway.
  (cd "$OUT" && mkdir Payload && cp -R Urushi.app Payload/ && zip -q -r -y Urushi.ipa Payload && rm -rf Payload)
  echo "built $OUT/Urushi.ipa (unsigned, version $VERSION)"
  exit 0
fi

# The simulator runs ad-hoc signed apps.
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "built $APP"
if [[ "$RUN" == true ]]; then
  DEVICE="${IOS_DEVICE:-iPhone 16 Pro}"
  xcrun simctl boot "$DEVICE" 2>/dev/null || true
  xcrun simctl install "$DEVICE" "$APP"
  xcrun simctl launch "$DEVICE" app.urushi.Urushi
  sleep 4
  xcrun simctl io "$DEVICE" screenshot build/ios-sim/screenshot.png
  echo "screenshot: build/ios-sim/screenshot.png"
fi
