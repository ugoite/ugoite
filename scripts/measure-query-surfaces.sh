#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${UGOITE_QUERY_MEASURE_OUTPUT:-$ROOT_DIR/target/query-surfaces-measurement.json}"
PROFILE_DIR="${UGOITE_CP1_PROFILE_DIR:-$ROOT_DIR/target/cp1-profiling}"
PROFILE_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
PROFILE_STARTED_MS="$(deno eval --quiet 'console.log(Date.now())')"
mkdir -p "$PROFILE_DIR"
PROFILE_REPORT="$PROFILE_DIR/query-${PROFILE_RUN_ID}.json"
SEED_A_PROFILE="$PROFILE_DIR/query-space-a-${PROFILE_RUN_ID}.json"
SEED_B_PROFILE="$PROFILE_DIR/query-space-b-${PROFILE_RUN_ID}.json"
WASM_BUILD_RESOURCE="$PROFILE_DIR/query-wasm-build-${PROFILE_RUN_ID}.time.txt"
SERVER_BUILD_RESOURCE="$PROFILE_DIR/query-server-build-${PROFILE_RUN_ID}.time.txt"
QUERY_E2E_RESOURCE="$PROFILE_DIR/query-playwright-${PROFILE_RUN_ID}.time.txt"
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
  local exit_code=$?
  trap - EXIT INT TERM
  set +e
  deno run -A "$ROOT_DIR/tools/cp1_profile.ts" query \
    --output "$PROFILE_REPORT" \
    --root "$MEASURE_ROOT" \
    --started-ms "$PROFILE_STARTED_MS" \
    --exit-code "$exit_code" \
    --evidence-report "$OUTPUT_FILE" \
    --seed query-space-a 3134001 6000 "Query Measurement Owner" \
      "$SEED_A_PROFILE" "${SEED_A_PROFILE%.json}.time.txt" \
    --seed query-space-b 3134002 4000 "Query Measurement Owner" \
      "$SEED_B_PROFILE" "${SEED_B_PROFILE%.json}.time.txt" \
    --timed-step wasm-build "$WASM_BUILD_RESOURCE" \
    --timed-step server-build "$SERVER_BUILD_RESOURCE" \
    --timed-step query-playwright "$QUERY_E2E_RESOURCE"
  local profile_exit_code=$?
  if [[ "$KEEP_ROOT" == false ]]; then
    rm -rf "$MEASURE_ROOT"
    local cleanup_exit_code=$?
    if [[ "$exit_code" -eq 0 && "$cleanup_exit_code" -ne 0 ]]; then
      exit_code=$cleanup_exit_code
    fi
  fi
  if [[ "$exit_code" -eq 0 && "$profile_exit_code" -ne 0 ]]; then exit_code=1; fi
  exit "$exit_code"
}
trap 'exit 130' INT
trap 'exit 143' TERM
trap cleanup EXIT

echo "Preparing fixed query measurement dataset..." >&2
bash "$ROOT_DIR/scripts/dev-seed.sh" \
  --root "$MEASURE_ROOT" \
  --space-id query-space-a \
  --owner "Query Measurement Owner" \
  --scenario renewable-ops \
  --entry-count 6000 \
  --seed 3134001 \
  --profile-output "$SEED_A_PROFILE"
bash "$ROOT_DIR/scripts/dev-seed.sh" \
  --root "$MEASURE_ROOT" \
  --space-id query-space-b \
  --owner "Query Measurement Owner" \
  --scenario renewable-ops \
  --entry-count 4000 \
  --seed 3134002 \
  --profile-output "$SEED_B_PROFILE"

echo "Running the server-backed browser measurement..." >&2
bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
  "$WASM_BUILD_RESOURCE" mise run build:wasm
bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
  "$SERVER_BUILD_RESOURCE" cargo build -p ugoite-server --locked
UGOITE_SOURCE_SHA="${UGOITE_SOURCE_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}" \
UGOITE_QUERY_MEASURE_ENABLED=true \
UGOITE_QUERY_MEASURE_OUTPUT="$OUTPUT_FILE" \
UGOITE_E2E_STARTUP_TIMEOUT_SECONDS=300 \
E2E_ENFORCE_CI_GATES="${E2E_ENFORCE_CI_GATES:-false}" \
E2E_STORAGE_ROOT="$MEASURE_ROOT" \
  bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
    "$QUERY_E2E_RESOURCE" bash "$ROOT_DIR/e2e/scripts/run-e2e.sh" query-measurement

echo "Measurement report: $OUTPUT_FILE" >&2
if [[ "$KEEP_ROOT" == true ]]; then
  echo "Seeded Space root kept at: $MEASURE_ROOT" >&2
fi
