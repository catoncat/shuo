#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RAW_TAG="${1:-${GITHUB_REF_NAME:-v0.1.1}}"
TAG="${RAW_TAG#refs/tags/}"
if [[ "$TAG" != v* ]]; then
    TAG="v$TAG"
fi
VERSION="${SHUO_VERSION:-${TAG#v}}"
BUILD_NUMBER="${SHUO_BUILD_NUMBER:-1}"
APP_PATH="$(SHUO_VERSION="$VERSION" SHUO_BUILD_NUMBER="$BUILD_NUMBER" bash "$ROOT/Packaging/build_shuo_app.sh" | tail -n 1)"
ASSET_BASE="Shuo-${TAG}-macos"
ZIP_PATH="$ROOT/dist/${ASSET_BASE}.zip"
SHA_PATH="$ZIP_PATH.sha256"

rm -f "$ZIP_PATH" "$SHA_PATH"
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$ZIP_PATH"
shasum -a 256 "$ZIP_PATH" > "$SHA_PATH"

echo "$ZIP_PATH"
echo "$SHA_PATH"
