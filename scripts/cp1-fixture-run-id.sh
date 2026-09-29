#!/usr/bin/env bash
# Print the CP1 fixture run ID the current consumer must bind to.
#
# Precedence is the explicit local identifier, then the workflow run IDs, then
# the producer record written by scripts/prepare-cp1-fixtures.sh for the same
# profiling directory. A stale bundle copied without its producer record (or
# without an explicit identifier) fails closed in the fixture loader instead
# of being accepted as a same-source local bundle.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE_DIR="${UGOITE_CP1_PROFILE_DIR:-$ROOT_DIR/target/cp1-profiling}"

run_id="${UGOITE_CP1_RUN_ID:-${UGOITE_CI_RUN_ID:-${GITHUB_RUN_ID:-}}}"
if [[ -z "$run_id" && -f "$PROFILE_DIR/.cp1-fixture-run-id" ]]; then
  run_id="$(tr -d '[:space:]' <"$PROFILE_DIR/.cp1-fixture-run-id")"
fi
if [[ -z "$run_id" ]]; then
  echo "CP1 fixture bundle load requires the producer run ID: set UGOITE_CP1_RUN_ID or prepare fixtures with scripts/prepare-cp1-fixtures.sh" >&2
  exit 1
fi
printf '%s\n' "$run_id"
