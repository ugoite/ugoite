#!/usr/bin/env bash

set -euo pipefail

if [ -f ./node_modules/vitest/vitest.mjs ]; then
  exit 0
fi

echo "Missing package-local Vitest install in $(pwd); running bun install --frozen-lockfile --ignore-scripts..."
bun install --frozen-lockfile --ignore-scripts
