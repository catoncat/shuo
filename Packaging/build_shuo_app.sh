#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Shuo"
APP_VERSION="${SHUO_VERSION:-0.1.0}"
BUILD_NUMBER="${SHUO_BUILD_NUMBER:-1}"
APP_BUNDLE="$ROOT/dist/${APP_NAME}.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
BIN_DIR="$RESOURCES/bin"
CONFIG_DIR="$RESOURCES/configs"
SOUNDS_DIR="$RESOURCES/sounds"
SWIFT_BIN="$ROOT/.build/release/shuo"
ENGINE_BIN="$ROOT/Engine/shuo-engine/target/release/shuo-engine"

mkdir -p "$ROOT/dist"
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS" "$BIN_DIR" "$CONFIG_DIR" "$SOUNDS_DIR"

pushd "$ROOT" >/dev/null
swift build -c release
cargo build --release --manifest-path Engine/shuo-engine/Cargo.toml
popd >/dev/null

cp "$SWIFT_BIN" "$MACOS/$APP_NAME"
cp "$SWIFT_BIN" "$BIN_DIR/shuo"
cp "$ENGINE_BIN" "$BIN_DIR/shuo-engine"
cp "$ROOT/configs/shuo.context.json" "$CONFIG_DIR/shuo.context.json"
ln -sf shuo.context.json "$CONFIG_DIR/hj_dictation.context.json"
cp "$ROOT/App/Resources/sounds/"*.caf "$SOUNDS_DIR/"
chmod +x "$MACOS/$APP_NAME" "$BIN_DIR/shuo" "$BIN_DIR/shuo-engine"

cat >"$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>local.envvar.shuo</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$APP_VERSION</string>
  <key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Shuo needs microphone access for dictation.</string>
  <key>NSAppleEventsUsageDescription</key><string>Shuo needs Accessibility/automation permissions to insert recognized text.</string>
</dict>
</plist>
EOF

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_BUNDLE" >/dev/null 2>&1 || true
fi

echo "$APP_BUNDLE"
