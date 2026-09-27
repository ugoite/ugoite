#!/usr/bin/env bash

set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <label> <command> [args...]" >&2
  exit 1
fi

label="$1"
shift

start_epoch="$(date +%s)"

finish_measurement() {
  local exit_code=$?
  local duration_seconds=$(( $(date +%s) - start_epoch ))
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf 'label=%s\nduration_seconds=%s\nexit_code=%s\n' \
      "$label" "$duration_seconds" "$exit_code" >>"$GITHUB_OUTPUT"
  fi
  printf '%s exited with status %s after %ss\n' "$label" "$exit_code" "$duration_seconds"
  return "$exit_code"
}
trap finish_measurement EXIT

"$@"
