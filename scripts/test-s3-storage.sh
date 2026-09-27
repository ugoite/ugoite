#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${UGOITE_S3_TEST_ENDPOINT:-}" || -z "${UGOITE_S3_TEST_BUCKET:-}" ]]; then
  echo "UGOITE_S3_TEST_ENDPOINT and UGOITE_S3_TEST_BUCKET must select a configured S3-compatible deployment backend" >&2
  exit 2
fi

export UGOITE_S3_TEST_REQUIRED=1
cargo test -p ugoite-storage --test s3_contract --locked
cargo test -p ugoite-iceberg --test s3_recovery --locked
