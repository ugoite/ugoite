#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FIXTURE_OUTPUT_ROOT="${UGOITE_CP1_FIXTURE_OUTPUT_ROOT:-$ROOT_DIR/target/cp1-fixtures}"
PROFILE_DIR="${UGOITE_CP1_PROFILE_DIR:-$ROOT_DIR/target/cp1-profiling}"
SEEDER="${UGOITE_SEED_XTASK_BINARY:-}"
if [[ -z "$SEEDER" ]]; then
  echo "UGOITE_SEED_XTASK_BINARY must name the source-matched xtask binary" >&2
  exit 1
fi
if [[ ! -f "$SEEDER" || -L "$SEEDER" || ! -x "$SEEDER" ]]; then
  echo "CP1 fixture seeder must be an executable regular file: $SEEDER" >&2
  exit 1
fi

mkdir -p "$FIXTURE_OUTPUT_ROOT" "$PROFILE_DIR"
PROFILE_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
FIXTURE_RUN_ID="${UGOITE_CP1_RUN_ID:-${UGOITE_CI_RUN_ID:-${GITHUB_RUN_ID:-}}}"
if [[ -z "$FIXTURE_RUN_ID" ]]; then
  FIXTURE_RUN_ID="local-${PROFILE_RUN_ID}"
fi
printf '%s\n' "$FIXTURE_RUN_ID" >"$PROFILE_DIR/.cp1-fixture-run-id"
SOURCE_SHA="$(git rev-parse HEAD)"
QUERY_ROOT="$(mktemp -d "$FIXTURE_OUTPUT_ROOT/.query-work.XXXXXX")"
EXPORT_ROOT="$(mktemp -d "$FIXTURE_OUTPUT_ROOT/.export-work.XXXXXX")"
cleanup() {
  local exit_code=$?
  trap - EXIT INT TERM
  set +e
  if [[ "$exit_code" -ne 0 ]]; then
    echo "CP1 fixture preparation failed; retaining seed roots for diagnosis:" >&2
    echo "  query: $QUERY_ROOT" >&2
    echo "  export: $EXPORT_ROOT" >&2
    exit "$exit_code"
  fi
  rm -rf "$QUERY_ROOT" "$EXPORT_ROOT"
  local cleanup_exit_code=$?
  if [[ "$exit_code" -eq 0 && "$cleanup_exit_code" -ne 0 ]]; then
    exit_code=$cleanup_exit_code
  fi
  exit "$exit_code"
}
trap 'exit 130' INT
trap 'exit 143' TERM
trap cleanup EXIT

seed_set() {
  local kind="$1"
  local root="$2"
  local rows
  rows="$(deno run --quiet tools/cp1_fixture_spec.ts "$kind")"
  while IFS=$'\t' read -r slug scenario seed entry_count owner_display_name; do
    [[ -n "$slug" ]] || continue
    local profile="$PROFILE_DIR/${slug}-${PROFILE_RUN_ID}.json"
    local resource="${profile%.json}.time.txt"
    local seed_args=(
      --root "$root"
      --space-id "$slug"
      --scenario "$scenario"
      --entry-count "$entry_count"
      --seed "$seed"
    )
    if [[ -n "$owner_display_name" ]]; then
      seed_args+=(--owner "$owner_display_name")
    fi
    echo "Preparing CP1 $kind fixture $slug ($entry_count Entries)..." >&2
    # dev-seed.sh already records the seed process resources next to the seed
    # profile when --profile-output is set; an outer wrapper here would
    # overwrite that same <profile>.time.txt record.
    env UGOITE_SEED_XTASK_BINARY="$SEEDER" \
      UGOITE_SOURCE_SHA="$SOURCE_SHA" \
      bash scripts/dev-seed.sh "${seed_args[@]}" --profile-output "$profile"
    mkdir -p "$root/.cp1-profiles"
    cp "$profile" "$root/.cp1-profiles/$slug.json"
    cp "$resource" "$root/.cp1-profiles/$slug.time.txt"
  done <<<"$rows"
}

seed_set query "$QUERY_ROOT"
seed_set export "$EXPORT_ROOT"

echo "Verifying and packaging CP1 query fixtures..." >&2
bash scripts/measure-process-resources.sh "$PROFILE_DIR/fixture-bundle-query-${PROFILE_RUN_ID}.time.txt" \
  deno run -A tools/cp1_fixture_bundle.ts create query \
    --root "$QUERY_ROOT" \
    --out "$FIXTURE_OUTPUT_ROOT/query" \
    --xtask "$SEEDER" \
    --source-sha "$SOURCE_SHA" \
    --run-id "$FIXTURE_RUN_ID"

echo "Verifying and packaging CP1 export fixture..." >&2
bash scripts/measure-process-resources.sh "$PROFILE_DIR/fixture-bundle-export-${PROFILE_RUN_ID}.time.txt" \
  deno run -A tools/cp1_fixture_bundle.ts create export \
    --root "$EXPORT_ROOT" \
    --out "$FIXTURE_OUTPUT_ROOT/export" \
    --xtask "$SEEDER" \
    --source-sha "$SOURCE_SHA" \
    --run-id "$FIXTURE_RUN_ID"

echo "Prepared source- and run-scoped CP1 fixture bundles at $FIXTURE_OUTPUT_ROOT" >&2
