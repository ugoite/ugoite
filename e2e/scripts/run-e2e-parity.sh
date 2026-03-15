#!/bin/bash
# Authoritative local E2E runner.
# Prefer the same docker-compose path as CI when Docker is available. Fall back
# to a production-style host runner with CI-equivalent Playwright gates when
# Docker is unavailable.

set -e

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if command -v docker >/dev/null 2>&1; then
  exec bash "$SCRIPT_DIR/run-e2e-compose.sh" "$TEST_TYPE"
fi

echo "docker not found; falling back to direct-process parity mode."
echo "This fallback still builds the production frontend and enforces CI-style Playwright gates."
export E2E_FRONTEND_MODE="${E2E_FRONTEND_MODE:-prod}"
export E2E_ENFORCE_CI_GATES=true
exec bash "$SCRIPT_DIR/run-e2e.sh" "$TEST_TYPE"
