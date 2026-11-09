#!/usr/bin/env bash
# Builds build/android/urushi.apk (arm64) with the Android SDK/NDK command-line tools:
# no Gradle. Signs it with a local debug key. Requires ANDROID_HOME (or the default SDK
# location), an NDK under $ANDROID_HOME/ndk, and a JDK (for keytool).
set -euo pipefail
cd "$(dirname "$0")/.."
SDK="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
NDK="${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* | sort -V | tail -1)}"
HOST_TAG="$(ls "$NDK/toolchains/llvm/prebuilt" | head -1)"
BIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)"
API=26
PROFILE="${PROFILE:-release}"

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
"$TOOLS/aapt2" link -o "$OUT/unsigned.apk" --manifest apps/android/AndroidManifest.xml -I "$PLATFORM/android.jar" "$OUT/res.zip"
(cd "$OUT" && zip -q -r unsigned.apk lib)
"$TOOLS/zipalign" -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"
KEY="build/android-debug.keystore"
if [[ ! -f "$KEY" ]]; then
  keytool -genkeypair -keystore "$KEY" -storepass android -keypass android -alias debug \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Urushi Debug" >/dev/null 2>&1
fi
"$TOOLS/apksigner" sign --ks "$KEY" --ks-pass pass:android --key-pass pass:android --out "$OUT/urushi.apk" "$OUT/aligned.apk"
rm -f "$OUT/unsigned.apk" "$OUT/aligned.apk" "$OUT/res.zip"
echo "built $OUT/urushi.apk"
