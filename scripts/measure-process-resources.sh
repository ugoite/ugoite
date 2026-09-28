#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <resource-log-path> <command> [args...]" >&2
  exit 1
fi

resource_log="$1"
shift
mkdir -p "$(dirname "$resource_log")"
if [[ "$(uname -s)" == "Darwin" ]]; then
  /usr/bin/time -l -o "$resource_log" "$@"
else
  /usr/bin/time -v -o "$resource_log" "$@"
fi
