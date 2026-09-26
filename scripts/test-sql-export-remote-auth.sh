#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET_ROOT="$CARGO_TARGET_DIR"
  if [[ "$TARGET_ROOT" != /* ]]; then
    TARGET_ROOT="$ROOT_DIR/$TARGET_ROOT"
  fi
else
  TARGET_ROOT="$ROOT_DIR/target"
fi
export UGOITE_E2E_STARTUP_TIMEOUT_SECONDS="${UGOITE_E2E_STARTUP_TIMEOUT_SECONDS:-1800}"

if [[ -z "${UGOITE_CLI_BIN:-}" ]]; then
  cargo build --locked -p ugoite-cli --bin ugoite
  UGOITE_CLI_BIN="$TARGET_ROOT/debug/ugoite"
fi

if [[ ! -x "$UGOITE_CLI_BIN" ]]; then
  echo "UGOITE_CLI_BIN must point to an executable ugoite CLI" >&2
  exit 2
fi

export UGOITE_CLI_BIN
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/rust}" \
  bash "$ROOT_DIR/scripts/build-ugoite-wasm.sh" release \
    "$ROOT_DIR/target/wasm/ugoite_wasm.release.wasm"
bash "$ROOT_DIR/scripts/activate-ugoite-wasm.sh" release
exec bash "$ROOT_DIR/e2e/scripts/run-e2e.sh" sql-export-remote-auth
