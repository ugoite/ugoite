#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${1:-debug}"

case "$PROFILE" in
  debug)
    SOURCE="$ROOT_DIR/target/wasm/ugoite_wasm.debug.wasm"
    ;;
  release)
    SOURCE="$ROOT_DIR/target/wasm/ugoite_wasm.release.wasm"
    ;;
  *)
    echo "usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

DESTINATION="$ROOT_DIR/frontend/src/lib/generated/ugoite_wasm.wasm"

if [[ ! -f "$SOURCE" ]]; then
  echo "WASM build output was not found: $SOURCE" >&2
  exit 1
fi

mkdir -p "$(dirname "$DESTINATION")"
cp "$SOURCE" "$DESTINATION"
echo "Activated $PROFILE WASM at $DESTINATION"
