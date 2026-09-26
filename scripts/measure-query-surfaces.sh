#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${UGOITE_QUERY_MEASURE_OUTPUT:-$ROOT_DIR/target/query-surfaces-measurement.json}"
KEEP_ROOT=false

if [[ -n "${UGOITE_QUERY_MEASURE_ROOT:-}" ]]; then
  MEASURE_ROOT="$UGOITE_QUERY_MEASURE_ROOT"
  KEEP_ROOT=true
  mkdir -p "$MEASURE_ROOT"
  if [[ -n "$(find "$MEASURE_ROOT" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    echo "Refusing to seed a non-empty query measurement root: $MEASURE_ROOT" >&2
    exit 1
  fi
else
  MEASURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-query-surfaces.XXXXXX")"
fi

if [[ "$OUTPUT_FILE" != /* ]]; then
  OUTPUT_FILE="$ROOT_DIR/$OUTPUT_FILE"
fi

cleanup() {
  if [[ "$KEEP_ROOT" == false ]]; then
    rm -rf "$MEASURE_ROOT"
  fi
}
trap cleanup EXIT INT TERM

echo "Preparing fixed query measurement dataset..." >&2
bash "$ROOT_DIR/scripts/dev-seed.sh" \
  --root "$MEASURE_ROOT" \
  --space-id query-space-a \
  --owner "Query Measurement Owner" \
  --scenario renewable-ops \
  --entry-count 6000 \
  --seed 3134001
bash "$ROOT_DIR/scripts/dev-seed.sh" \
  --root "$MEASURE_ROOT" \
  --space-id query-space-b \
  --owner "Query Measurement Owner" \
  --scenario renewable-ops \
  --entry-count 4000 \
  --seed 3134002

echo "Running the server-backed browser measurement..." >&2
mise run build:wasm
cargo build -p ugoite-server --locked
UGOITE_SOURCE_SHA="${UGOITE_SOURCE_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}" \
UGOITE_QUERY_MEASURE_ENABLED=true \
UGOITE_QUERY_MEASURE_OUTPUT="$OUTPUT_FILE" \
UGOITE_E2E_STARTUP_TIMEOUT_SECONDS=300 \
E2E_STORAGE_ROOT="$MEASURE_ROOT" \
  bash "$ROOT_DIR/e2e/scripts/run-e2e.sh" query-measurement

echo "Measurement report: $OUTPUT_FILE" >&2
if [[ "$KEEP_ROOT" == true ]]; then
  echo "Seeded Space root kept at: $MEASURE_ROOT" >&2
fi
