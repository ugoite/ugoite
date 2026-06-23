#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${1:-debug}"

case "$PROFILE" in
  debug)
    CARGO_PROFILE_ARGS=()
    PROFILE_DIR="debug"
    ;;
  release)
    CARGO_PROFILE_ARGS=(--release)
    PROFILE_DIR="release"
    ;;
  *)
    echo "usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

cd "$ROOT_DIR"
build_args=(
  -p ugoite-wasm
  --target wasm32-unknown-unknown
  --locked
)
if [[ "${#CARGO_PROFILE_ARGS[@]}" -gt 0 ]]; then
  build_args+=("${CARGO_PROFILE_ARGS[@]}")
fi

cargo build "${build_args[@]}"

TARGET_ROOT="${CARGO_TARGET_DIR:-target/rust}"
if [[ "$TARGET_ROOT" != /* ]]; then
  TARGET_ROOT="$ROOT_DIR/$TARGET_ROOT"
fi
SOURCE="$TARGET_ROOT/wasm32-unknown-unknown/$PROFILE_DIR/ugoite_wasm.wasm"
DESTINATION="$ROOT_DIR/frontend/src/lib/generated/ugoite_wasm.wasm"

if [[ ! -f "$SOURCE" ]]; then
  echo "WASM build succeeded but output was not found: $SOURCE" >&2
  exit 1
fi

mkdir -p "$(dirname "$DESTINATION")"
cp "$SOURCE" "$DESTINATION"
echo "Wrote $DESTINATION"
