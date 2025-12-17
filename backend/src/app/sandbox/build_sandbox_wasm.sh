#!/usr/bin/env bash
set -euo pipefail

# Builds the WebAssembly sandbox from `runner.js` using Javy.
# Output: `backend/src/app/sandbox/sandbox.wasm`

SANDBOX_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SANDBOX_DIR/../../../../.." && pwd)"

JAVY_BIN="${JAVY_BIN:-javy}"

if ! command -v "$JAVY_BIN" >/dev/null 2>&1; then
  echo "javy not found on PATH." >&2
  echo "Install it with: bash $ROOT_DIR/scripts/setup_javy.sh" >&2
  exit 1
fi

cd "$SANDBOX_DIR"

# Javy can emit dynamic or static wasm; static is easier to run in Wasmtime.
"$JAVY_BIN" compile runner.js -o sandbox.wasm

echo "Built: $SANDBOX_DIR/sandbox.wasm"