#!/usr/bin/env bash
# Fail-closed preflight for the CP1 consumer lanes (cp1-query / cp1-export).
#
# Consumer jobs must reuse the fixtures-job bundles, the fixtures-job seeder,
# and the release artifacts built by artifact-build. A silent fallback to a
# local dev-seed, a host cargo build, a locally rebuilt image, or a debug CLI
# would hide a broken producer, so every fallback path fails here instead.
#
# Usage: assert-cp1-consumer-inputs.sh <query|export>
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: assert-cp1-consumer-inputs.sh <query|export>" >&2
  exit 1
fi
KIND="$1"
if [[ "$KIND" != "query" && "$KIND" != "export" ]]; then
  echo "usage: assert-cp1-consumer-inputs.sh <query|export>" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  echo "CP1 $KIND consumer preflight: $1" >&2
  exit 1
}

resolve_root_relative() {
  local path="$1"
  if [[ "$path" == /* ]]; then
    printf '%s\n' "$path"
  else
    printf '%s/%s\n' "$ROOT_DIR" "$path"
  fi
}

SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [[ "${UGOITE_SOURCE_SHA:-}" != "$SOURCE_SHA" ]]; then
  fail "UGOITE_SOURCE_SHA must match the checked out source ($SOURCE_SHA)"
fi

# The fixture loader binds restored bundles to the producer workflow run, so
# resolve the expected run ID through the shared helper (explicit identifiers
# first, then the fixtures-job producer record). The helper exits non-zero
# when no identifier is available, which fails this preflight closed.
if ! RESOLVED_CP1_RUN_ID="$(bash "$ROOT_DIR/scripts/cp1-fixture-run-id.sh")"; then
  fail "a fixture producer run ID (UGOITE_CP1_RUN_ID, UGOITE_CI_RUN_ID, or GITHUB_RUN_ID) is required"
fi
export UGOITE_CP1_RUN_ID="$RESOLVED_CP1_RUN_ID"

# The fixtures-job seeder (binary plus its source-SHA sidecar) is the only
# verifier consumers may use; there is no cargo rebuild in consumer jobs.
SEEDER="${UGOITE_SEED_XTASK_BINARY:?UGOITE_SEED_XTASK_BINARY must point at the fixtures-job seeder}"
SEEDER="$(resolve_root_relative "$SEEDER")"
[[ -f "$SEEDER" && ! -L "$SEEDER" ]] || fail "seeder is not a regular file: $SEEDER"
[[ -f "$SEEDER.source-sha" && ! -L "$SEEDER.source-sha" ]] \
  || fail "seeder is missing its source SHA sidecar: $SEEDER.source-sha"
[[ "$(tr -d '[:space:]' <"$SEEDER.source-sha")" == "$SOURCE_SHA" ]] \
  || fail "seeder source SHA does not match the checked out source"
chmod u+x "$SEEDER"
[[ -x "$SEEDER" ]] || fail "seeder is not executable: $SEEDER"

# The prepared bundle directory is the only dataset consumers may measure;
# an unset directory would silently fall back to a local dev-seed.
BUNDLE_DIR="${UGOITE_CP1_FIXTURE_BUNDLE_DIR:?UGOITE_CP1_FIXTURE_BUNDLE_DIR must point at the downloaded $KIND fixture bundle}"
BUNDLE_DIR="$(resolve_root_relative "$BUNDLE_DIR")"
[[ -d "$BUNDLE_DIR" && ! -L "$BUNDLE_DIR" ]] || fail "fixture bundle is not a directory: $BUNDLE_DIR"
[[ -f "$BUNDLE_DIR/manifest.json" && ! -L "$BUNDLE_DIR/manifest.json" ]] \
  || fail "fixture bundle is missing its manifest: $BUNDLE_DIR/manifest.json"
[[ -f "$BUNDLE_DIR/$KIND-fixtures.tar.gz" && ! -L "$BUNDLE_DIR/$KIND-fixtures.tar.gz" ]] \
  || fail "fixture bundle is missing its archive: $BUNDLE_DIR/$KIND-fixtures.tar.gz"

if [[ "$KIND" == "query" ]]; then
  # The query consumer must reuse the preloaded release image through the
  # Compose runner; the host runner would cargo-build WASM and the server.
  [[ "${UGOITE_QUERY_MEASURE_RUNNER:-}" == "compose" ]] \
    || fail "UGOITE_QUERY_MEASURE_RUNNER must be 'compose' in CI (host mode rebuilds WASM and the server)"
  [[ "${E2E_BUILD_IMAGES:-}" == "false" ]] \
    || fail "E2E_BUILD_IMAGES must be 'false' in CI (the release image is preloaded)"
  IMAGE_TAG="${UGOITE_IMAGE_TAG:-ugoite:e2e}"
  docker image inspect "$IMAGE_TAG" >/dev/null \
    || fail "verified release image is not loaded: $IMAGE_TAG"
  # Restoration must target a fresh unique root, never a caller-kept directory.
  [[ -z "${UGOITE_QUERY_MEASURE_ROOT:-}" ]] \
    || fail "UGOITE_QUERY_MEASURE_ROOT must be unset in CI so fixtures restore to a unique temp root"
else
  # The export consumer must use the verified release CLI; an unset binary
  # would silently fall back to a locally built debug CLI.
  CLI_BIN="${UGOITE_SQL_EXPORT_CLI_BINARY:?UGOITE_SQL_EXPORT_CLI_BINARY must point at the verified release CLI}"
  CLI_BIN="$(resolve_root_relative "$CLI_BIN")"
  [[ -f "$CLI_BIN" && ! -L "$CLI_BIN" && -x "$CLI_BIN" ]] \
    || fail "verified release CLI is not an executable regular file: $CLI_BIN"
  [[ -n "${UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE:-}" && -n "${UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT:-}" ]] \
    || fail "SQL export CLI artifact load resource and transfer report must be configured in CI"
  [[ -z "${UGOITE_SQL_EXPORT_MEASURE_ROOT:-}" ]] \
    || fail "UGOITE_SQL_EXPORT_MEASURE_ROOT must be unset in CI so fixtures restore to a unique temp root"
fi

echo "CP1 $KIND consumer preflight: prepared fixtures, seeder, and release inputs verified" >&2
