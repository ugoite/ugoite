#!/usr/bin/env bash
set -euo pipefail

# S3 recovery fixtures (issue #3266) live under unique
# `ugoite/l12/<suite>/<uuid>` prefixes and are best-effort removed after
# each run; still, point these suites at a dedicated test bucket, since
# interrupted runs may leave orphaned prefixes behind.
if [[ -z "${UGOITE_S3_TEST_ENDPOINT:-}" || -z "${UGOITE_S3_TEST_BUCKET:-}" ]]; then
  echo "UGOITE_S3_TEST_ENDPOINT and UGOITE_S3_TEST_BUCKET must select a configured S3-compatible deployment backend" >&2
  exit 2
fi

export UGOITE_S3_TEST_REQUIRED=1
cargo test -p ugoite-storage --test s3_contract --locked
# Run the two-process acceptance serially. The tests share one process-global
# Space-creation serializer and authorization write lock by design, and each
# race holds its setup state across 60s-bounded gate waits; parallel libtest
# threads convoy on those locks and starve gate progress. Serial execution
# keeps the gate sequencing deterministic.
cargo test -p ugoite-iceberg --test s3_recovery --locked -- --test-threads=1
