#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${UGOITE_QUERY_MEASURE_OUTPUT:-$ROOT_DIR/target/query-surfaces-measurement.json}"
PROFILE_DIR="${UGOITE_CP1_PROFILE_DIR:-$ROOT_DIR/target/cp1-profiling}"
PROFILE_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
PROFILE_STARTED_MS="$(deno eval --quiet 'console.log(Date.now())')"
mkdir -p "$PROFILE_DIR"
PROFILE_REPORT="$PROFILE_DIR/query-${PROFILE_RUN_ID}.json"
WASM_BUILD_RESOURCE="$PROFILE_DIR/query-wasm-build-${PROFILE_RUN_ID}.time.txt"
SERVER_BUILD_RESOURCE="$PROFILE_DIR/query-server-build-${PROFILE_RUN_ID}.time.txt"
QUERY_E2E_RESOURCE="$PROFILE_DIR/query-playwright-${PROFILE_RUN_ID}.time.txt"
QUERY_FIXTURE_ROWS="$(deno run --quiet "$ROOT_DIR/tools/cp1_fixture_spec.ts" query)"
declare -a QUERY_FIXTURE_SLUGS=()
declare -a QUERY_FIXTURE_SCENARIOS=()
declare -a QUERY_FIXTURE_SEEDS=()
declare -a QUERY_FIXTURE_COUNTS=()
declare -a QUERY_FIXTURE_OWNERS=()
declare -a QUERY_PROFILE_ARGS=()
FIXTURE_BUNDLE_DIR="${UGOITE_CP1_FIXTURE_BUNDLE_DIR:-}"
if [[ -n "$FIXTURE_BUNDLE_DIR" && "$FIXTURE_BUNDLE_DIR" != /* ]]; then
  FIXTURE_BUNDLE_DIR="$ROOT_DIR/$FIXTURE_BUNDLE_DIR"
fi
while IFS=$'\t' read -r fixture_slug scenario seed entry_count owner_display_name; do
  [[ -n "$fixture_slug" ]] || continue
  if ! [[ "$seed" =~ ^[0-9]+$ && "$entry_count" =~ ^[0-9]+$ ]]; then
    echo "Invalid numeric values in CP1 query fixture spec for $fixture_slug" >&2
    exit 1
  fi
  QUERY_FIXTURE_SLUGS+=("$fixture_slug")
  QUERY_FIXTURE_SCENARIOS+=("$scenario")
  QUERY_FIXTURE_SEEDS+=("$seed")
  QUERY_FIXTURE_COUNTS+=("$entry_count")
  QUERY_FIXTURE_OWNERS+=("$owner_display_name")
done <<<"$QUERY_FIXTURE_ROWS"
if [[ "${#QUERY_FIXTURE_SLUGS[@]}" -eq 0 ]]; then
  echo "CP1 query fixture specification is empty" >&2
  exit 1
fi
KEEP_ROOT=false
QUERY_MEASURE_RUNNER="${UGOITE_QUERY_MEASURE_RUNNER:-host}"
QUERY_E2E_PROFILE_STEP="query-playwright"
case "$QUERY_MEASURE_RUNNER" in
  host) ;;
  compose) QUERY_E2E_PROFILE_STEP="query-playwright-compose" ;;
  *)
    echo "UGOITE_QUERY_MEASURE_RUNNER must be 'host' or 'compose': $QUERY_MEASURE_RUNNER" >&2
    exit 1
    ;;
esac

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

SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
for fixture_slug in "${QUERY_FIXTURE_SLUGS[@]}"; do
  if [[ -n "$FIXTURE_BUNDLE_DIR" ]]; then
    fixture_profile="$MEASURE_ROOT/.cp1-profiles/$fixture_slug.json"
  else
    fixture_profile="$PROFILE_DIR/${fixture_slug}-${PROFILE_RUN_ID}.json"
  fi
  fixture_resource="${fixture_profile%.json}.time.txt"
  QUERY_PROFILE_ARGS+=(--seed "$fixture_slug" "$fixture_profile" "$fixture_resource")
done

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
    "${QUERY_PROFILE_ARGS[@]}" \
    --timed-step wasm-build "$WASM_BUILD_RESOURCE" \
    --timed-step server-build "$SERVER_BUILD_RESOURCE" \
    --timed-step "$QUERY_E2E_PROFILE_STEP" "$QUERY_E2E_RESOURCE"
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
if [[ -n "$FIXTURE_BUNDLE_DIR" ]]; then
  deno run -A "$ROOT_DIR/tools/cp1_fixture_bundle.ts" load query \
    --bundle-dir "$FIXTURE_BUNDLE_DIR" \
    --destination "$MEASURE_ROOT" \
    --xtask "${UGOITE_SEED_XTASK_BINARY:-}" \
    --source-sha "$SOURCE_SHA"
else
  for ((index = 0; index < ${#QUERY_FIXTURE_SLUGS[@]}; index++)); do
    fixture_slug="${QUERY_FIXTURE_SLUGS[$index]}"
    fixture_profile="$PROFILE_DIR/${fixture_slug}-${PROFILE_RUN_ID}.json"
    seed_args=(
      --root "$MEASURE_ROOT"
      --space-id "$fixture_slug"
      --scenario "${QUERY_FIXTURE_SCENARIOS[$index]}"
      --entry-count "${QUERY_FIXTURE_COUNTS[$index]}"
      --seed "${QUERY_FIXTURE_SEEDS[$index]}"
    )
    if [[ -n "${QUERY_FIXTURE_OWNERS[$index]}" ]]; then
      seed_args+=(--owner "${QUERY_FIXTURE_OWNERS[$index]}")
    fi
    bash "$ROOT_DIR/scripts/dev-seed.sh" \
      "${seed_args[@]}" \
      --profile-output "$fixture_profile"
  done
fi

echo "Running the server-backed browser measurement..." >&2
if [[ "$QUERY_MEASURE_RUNNER" == "host" ]]; then
  bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
    "$WASM_BUILD_RESOURCE" mise run build:wasm
  bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
    "$SERVER_BUILD_RESOURCE" cargo build -p ugoite-server --locked
  UGOITE_SOURCE_SHA="${UGOITE_SOURCE_SHA:-$SOURCE_SHA}" \
  UGOITE_QUERY_MEASURE_ENABLED=true \
  UGOITE_QUERY_MEASURE_OUTPUT="$OUTPUT_FILE" \
  UGOITE_E2E_STARTUP_TIMEOUT_SECONDS=300 \
  E2E_ENFORCE_CI_GATES="${E2E_ENFORCE_CI_GATES:-false}" \
  E2E_STORAGE_ROOT="$MEASURE_ROOT" \
    bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
      "$QUERY_E2E_RESOURCE" bash "$ROOT_DIR/e2e/scripts/run-e2e.sh" query-measurement
else
  UGOITE_SOURCE_SHA="${UGOITE_SOURCE_SHA:-$SOURCE_SHA}" \
  UGOITE_QUERY_MEASURE_ENABLED=true \
  UGOITE_QUERY_MEASURE_OUTPUT="$OUTPUT_FILE" \
  E2E_BUILD_IMAGES="${E2E_BUILD_IMAGES:-true}" \
  E2E_BACKEND_START_TIMEOUT_SECONDS=300 \
  E2E_READINESS_TIMEOUT_SECONDS=300 \
  E2E_STORAGE_ROOT="$MEASURE_ROOT" \
    bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
      "$QUERY_E2E_RESOURCE" bash "$ROOT_DIR/e2e/scripts/run-e2e-compose.sh" \
        query-measurement --fixture-root "$MEASURE_ROOT"
fi

echo "Measurement report: $OUTPUT_FILE" >&2
if [[ "$KEEP_ROOT" == true ]]; then
  echo "Seeded Space root kept at: $MEASURE_ROOT" >&2
fi
