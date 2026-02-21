#!/usr/bin/env bash
set -euo pipefail

config_path="${HOME}/.ugoite/cli-endpoints.json"

if [ -f "${config_path}" ]; then
  rm -f "${config_path}"
  echo "Removed ${config_path}"
else
  echo "No endpoint config found at ${config_path}"
fi

echo "CLI endpoint routing config reset complete"
