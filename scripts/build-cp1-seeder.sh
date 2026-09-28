#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/rust}"
if [[ "$CARGO_TARGET_DIR" != /* ]]; then
  CARGO_TARGET_DIR="$ROOT_DIR/$CARGO_TARGET_DIR"
fi

SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [[ ! "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Could not resolve the checked out source SHA for the CP1 seeder" >&2
  exit 1
fi
if [[ -n "${UGOITE_SOURCE_SHA:-}" && "$UGOITE_SOURCE_SHA" != "$SOURCE_SHA" ]]; then
  echo "UGOITE_SOURCE_SHA must match the checked out source before building the CP1 seeder" >&2
  exit 1
fi

cd "$ROOT_DIR"
env CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --locked -p xtask

SEEDER_BINARY="$CARGO_TARGET_DIR/debug/xtask"
if [[ ! -f "$SEEDER_BINARY" || -L "$SEEDER_BINARY" || ! -x "$SEEDER_BINARY" ]]; then
  echo "cargo build did not produce an executable CP1 seeder: $SEEDER_BINARY" >&2
  exit 1
fi

SIDECAR="$SEEDER_BINARY.source-sha"
SIDECAR_TMP="$SIDECAR.tmp.$$"
cleanup() {
  rm -f "$SIDECAR_TMP"
}
trap cleanup EXIT INT TERM
printf '%s\n' "$SOURCE_SHA" >"$SIDECAR_TMP"
mv -f "$SIDECAR_TMP" "$SIDECAR"
trap - EXIT INT TERM

echo "Built CP1 seeder for source $SOURCE_SHA at $SEEDER_BINARY" >&2
