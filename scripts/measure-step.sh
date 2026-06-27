#!/usr/bin/env bash

set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <label> <command> [args...]" >&2
  exit 1
fi

label="$1"
shift

start_epoch="$(date +%s)"
"$@"
end_epoch="$(date +%s)"
duration_seconds="$((end_epoch - start_epoch))"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'duration_seconds=%s\n' "$duration_seconds" >>"$GITHUB_OUTPUT"
fi

printf '%s completed in %ss\n' "$label" "$duration_seconds"
