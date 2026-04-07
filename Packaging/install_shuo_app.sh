#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="${DEST_DIR:-$HOME/Applications}"
APP_NAME="Shuo.app"

APP_PATH="$(bash "$ROOT/Packaging/build_shuo_app.sh" | tail -n 1)"
mkdir -p "$DEST_DIR"
rm -rf "$DEST_DIR/$APP_NAME"
ditto "$APP_PATH" "$DEST_DIR/$APP_NAME"

echo "$DEST_DIR/$APP_NAME"
