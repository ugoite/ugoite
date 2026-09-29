#!/usr/bin/env bash
# Stage the fixtures-job CP1 seeder binary and its source-SHA sidecar for
# consumer download. The seeder is built once (scripts/build-cp1-seeder.sh)
# and reused by the query/export consumers for post-restore verification, so
# consumer jobs never rebuild it.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SEEDER="${UGOITE_SEED_XTASK_BINARY:?UGOITE_SEED_XTASK_BINARY must name the source-matched xtask binary}"
if [[ "$SEEDER" != /* ]]; then
  SEEDER="$ROOT_DIR/$SEEDER"
fi
if [[ ! -f "$SEEDER" || -L "$SEEDER" || ! -x "$SEEDER" ]]; then
  echo "CP1 fixture seeder must be an executable regular file: $SEEDER" >&2
  exit 1
fi
if [[ ! -f "$SEEDER.source-sha" || -L "$SEEDER.source-sha" ]]; then
  echo "CP1 fixture seeder is missing its source SHA sidecar: $SEEDER.source-sha" >&2
  exit 1
fi
SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [[ "$(tr -d '[:space:]' <"$SEEDER.source-sha")" != "$SOURCE_SHA" ]]; then
  echo "CP1 fixture seeder source SHA does not match the checked out source" >&2
  exit 1
fi
if [[ -n "${UGOITE_SOURCE_SHA:-}" && "$UGOITE_SOURCE_SHA" != "$SOURCE_SHA" ]]; then
  echo "UGOITE_SOURCE_SHA must match the checked out source before staging the CP1 seeder" >&2
  exit 1
fi

STAGE_DIR="$ROOT_DIR/target/cp1-fixtures/seeder"
rm -rf "$STAGE_DIR"
mkdir -p -m 700 "$STAGE_DIR"
cp "$SEEDER" "$STAGE_DIR/xtask"
cp "$SEEDER.source-sha" "$STAGE_DIR/xtask.source-sha"
chmod 755 "$STAGE_DIR/xtask"
chmod 644 "$STAGE_DIR/xtask.source-sha"

echo "Staged CP1 seeder for consumer jobs at $STAGE_DIR" >&2
