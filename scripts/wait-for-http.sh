#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <url> [timeout-seconds]" >&2
  exit 1
fi

url="$1"
timeout_seconds="${2:-30}"

if ! [[ "$timeout_seconds" =~ ^[0-9]+$ ]] || [ "$timeout_seconds" -le 0 ]; then
  echo "timeout-seconds must be a positive integer" >&2
  exit 1
fi

deadline=$((SECONDS + timeout_seconds))

while true; do
  if curl -fsS "$url" >/dev/null 2>&1; then
    exit 0
  fi

  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "Timed out waiting for $url" >&2
    exit 1
  fi

  sleep 0.2
done
