#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="HJ Voice"
APP_BUNDLE="$ROOT/dist/${APP_NAME}.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
BIN_DIR="$RESOURCES/bin"
CONFIG_DIR="$RESOURCES/configs"
SOUNDS_DIR="$RESOURCES/sounds"
SWIFT_BIN="$ROOT/.build/release/hj-voice"
ENGINE_BIN="$ROOT/Engine/hj-dictation/target/release/hj-dictation"

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS" "$BIN_DIR" "$CONFIG_DIR" "$SOUNDS_DIR"

pushd "$ROOT" >/dev/null
swift build -c release
cargo build --release --manifest-path Engine/hj-dictation/Cargo.toml
popd >/dev/null

cp "$SWIFT_BIN" "$MACOS/$APP_NAME"
cp "$SWIFT_BIN" "$BIN_DIR/hj-voice"
cp "$ENGINE_BIN" "$BIN_DIR/hj-dictation"
cp "$ROOT/configs/hj_dictation.context.json" "$CONFIG_DIR/hj_dictation.context.json"
cp "$ROOT/App/Resources/sounds/"*.caf "$SOUNDS_DIR/"
chmod +x "$MACOS/$APP_NAME" "$BIN_DIR/hj-voice" "$BIN_DIR/hj-dictation"

cat >"$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>local.envvar.hjvoice</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>HJ Voice needs microphone access for dictation.</string>
  <key>NSAppleEventsUsageDescription</key><string>HJ Voice needs Accessibility/automation permissions to insert recognized text.</string>
</dict>
</plist>
EOF

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_BUNDLE" >/dev/null 2>&1 || true
fi

echo "$APP_BUNDLE"
