#!/usr/bin/env bash
# Builds build/android/urushi.apk (arm64) with the Android SDK/NDK command-line tools:
# no Gradle. Requires ANDROID_HOME (or the default macOS SDK location), an NDK under
# $ANDROID_HOME/ndk (or ANDROID_NDK_HOME), and a JDK (for keytool and apksigner). Runs on
# macOS and Linux.
#
# The APK's versionName is [workspace.package].version from Cargo.toml, and its
# versionCode is derived from it (MAJOR*1000000 + MINOR*1000 + PATCH), so every release
# installs as an update over the previous one.
#
# Signing: with ANDROID_KEYSTORE set, signs with that release keystore
# (ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS, and ANDROID_KEY_PASSWORD, which defaults
# to the keystore password). Otherwise signs with Android's standard debug key,
# ~/.android/debug.keystore (shared with Android Studio, created on first use), so builds
# from every worktree install over each other. Android only installs an update signed
# with the same key as the installed app.
set -euo pipefail
cd "$(dirname "$0")/.."
SDK="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
NDK="${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* | sort -V | tail -1)}"
HOST_TAG="$(ls "$NDK/toolchains/llvm/prebuilt" | head -1)"
BIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
# Numbered platforms only: preview ones (e.g. android-Baklava) would sort after them.
PLATFORM="$(ls -d "$SDK"/platforms/android-[0-9]* | sort -V | tail -1)"
API=26
PROFILE="${PROFILE:-release}"

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
IFS=. read -r MAJOR MINOR PATCH <<<"${VERSION%%-*}"
VERSION_CODE=$((MAJOR * 1000000 + MINOR * 1000 + PATCH))

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$BIN/aarch64-linux-android$API-clang"
export CC_aarch64_linux_android="$BIN/aarch64-linux-android$API-clang"
export AR_aarch64_linux_android="$BIN/llvm-ar"
cargo build --profile "$PROFILE" --target aarch64-linux-android -p urushi-android

OUT=build/android
rm -rf "$OUT" && mkdir -p "$OUT/lib/arm64-v8a"
# Keep build outputs out of Spotlight/Launchpad (only /Applications/Urushi.app should show).
mkdir -p build && touch build/.metadata_never_index
PROFILE_DIR="$PROFILE"; [[ "$PROFILE" == "dev" ]] && PROFILE_DIR=debug
cp "target/aarch64-linux-android/$PROFILE_DIR/liburushi.so" "$OUT/lib/arm64-v8a/"
"$BIN/llvm-strip" "$OUT/lib/arm64-v8a/liburushi.so"
# The launcher icon (apps/android/res, from scripts/render-icons.swift).
"$TOOLS/aapt2" compile --dir apps/android/res -o "$OUT/res.zip"
"$TOOLS/aapt2" link -o "$OUT/unsigned.apk" --manifest apps/android/AndroidManifest.xml \
  -I "$PLATFORM/android.jar" --version-name "$VERSION" --version-code "$VERSION_CODE" \
  --replace-version "$OUT/res.zip"
(cd "$OUT" && zip -q -r unsigned.apk lib)
"$TOOLS/zipalign" -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"

if [[ -n "${ANDROID_KEYSTORE:-}" ]]; then
  export ANDROID_KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-${ANDROID_KEYSTORE_PASSWORD:?ANDROID_KEYSTORE_PASSWORD is required with ANDROID_KEYSTORE}}"
  # Passwords are read from the environment so they never appear in a process listing.
  "$TOOLS/apksigner" sign --ks "$ANDROID_KEYSTORE" --ks-key-alias "${ANDROID_KEY_ALIAS:?ANDROID_KEY_ALIAS is required with ANDROID_KEYSTORE}" \
    --ks-pass env:ANDROID_KEYSTORE_PASSWORD --key-pass env:ANDROID_KEY_PASSWORD \
    --out "$OUT/urushi.apk" "$OUT/aligned.apk"
  SIGNED_WITH="release key ($ANDROID_KEYSTORE)"
else
  KEY="$HOME/.android/debug.keystore"
  if [[ ! -f "$KEY" ]]; then
    mkdir -p "$HOME/.android"
    keytool -genkeypair -keystore "$KEY" -storepass android -keypass android \
      -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
      -dname "CN=Android Debug,O=Android,C=US" >/dev/null 2>&1
  fi
  "$TOOLS/apksigner" sign --ks "$KEY" --ks-key-alias androiddebugkey \
    --ks-pass pass:android --key-pass pass:android \
    --out "$OUT/urushi.apk" "$OUT/aligned.apk"
  SIGNED_WITH="debug key ($KEY)"
fi
rm -f "$OUT/unsigned.apk" "$OUT/aligned.apk" "$OUT/res.zip"
echo "built $OUT/urushi.apk (version $VERSION, code $VERSION_CODE, signed with $SIGNED_WITH)"
