#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:-$ROOT/../hj-app}"

mkdir -p "$DEST"

rsync -a --delete \
  "$ROOT/App" \
  "$ROOT/Engine" \
  "$ROOT/Shared" \
  "$ROOT/Packaging" \
  "$ROOT/Package.swift" \
  "$ROOT/configs" \
  "$DEST/"

cat >"$DEST/README.md" <<'EOF'
# hj-app

- Swift host: `App/Sources`
- Rust helper: `Engine/hj-dictation`
- Shared contract: `Shared`
- Packaging: `Packaging`

## build

- `swift build`
- `cargo build --manifest-path Engine/hj-dictation/Cargo.toml`
- `bash Packaging/build_hj_app.sh`
EOF

cat >"$DEST/.gitignore" <<'EOF'
.build/
dist/
DerivedData/
Engine/hj-dictation/target/
*.xcuserstate
.DS_Store
EOF

if [ ! -d "$DEST/.git" ]; then
  git -C "$DEST" init >/dev/null
fi

echo "$DEST"
