#!/bin/bash
# Direct-process E2E runner for fast local iteration and for no-Docker parity
# fallback via `run-e2e-parity.sh`.
#
# Usage: ./e2e/scripts/run-e2e.sh [test-type]
#   test-type: "smoke", "entries", "screenshot", or "full" (runs standard tests)
#
# Environment variables:
#   E2E_TEST_TIMEOUT_MS: per-test timeout passed to `playwright test --timeout`
#   E2E_FRONTEND_MODE: "dev" (default) or "prod" to use build+start for SSR speed
#   E2E_ENFORCE_CI_GATES: "true" to emit JUnit output and fail on skipped tests

set -e

# Unset VIRTUAL_ENV to ensure we're using the environment managed by mise/uv
# and not inheriting an active virtualenv from the current shell session.
unset VIRTUAL_ENV
export BASELINE_BROWSER_MAPPING_IGNORE_OLD_DATA=true
export BROWSERSLIST_IGNORE_OLD_DATA=true

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-e2e-local-signing-secret}"
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
STATIC_E2E_TOKENS_JSON='{"alice-token":{"user_id":"alice-user","principal_type":"user"},"bob-token":{"user_id":"bob-user","principal_type":"user"}}'
ENFORCE_CI_GATES="${E2E_ENFORCE_CI_GATES:-false}"

if [ "$ENFORCE_CI_GATES" = "true" ]; then
  export PLAYWRIGHT_CI_REPORTER=junit
  export PLAYWRIGHT_JUNIT_OUTPUT_FILE="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"
fi

echo "Checking for existing processes on ports 8000 and 3000..."
fuser -k 8000/tcp 2>/dev/null || true
fuser -k 3000/tcp 2>/dev/null || true
sleep 1

echo "Creating default space..."
cd "$ROOT_DIR/backend"
E2E_STORAGE_ROOT="${E2E_STORAGE_ROOT:-}"
if [ -z "$E2E_STORAGE_ROOT" ]; then
  E2E_STORAGE_ROOT="/tmp/ugoite-e2e"
  CLEANUP_E2E_STORAGE=true
else
  CLEANUP_E2E_STORAGE=false
fi

mkdir -p "$E2E_STORAGE_ROOT"

UGOITE_ROOT="$E2E_STORAGE_ROOT" uv run python - <<'PY'
import asyncio

import ugoite_core

from app.core.config import get_root_path
from app.core.storage import storage_config_from_root


async def main() -> None:
    config = storage_config_from_root(get_root_path())
    try:
        await ugoite_core.create_space(config, "default")
    except RuntimeError as exc:
        if "already exists" not in str(exc).lower():
            raise


asyncio.run(main())
PY

echo "Starting backend server..."
cd "$ROOT_DIR/backend"
UGOITE_ROOT="$E2E_STORAGE_ROOT" \
  UGOITE_ALLOW_REMOTE=true \
  UGOITE_DEV_AUTH_MODE=mock-oauth \
  UGOITE_DEV_USER_ID=e2e-user \
  UGOITE_DEV_SIGNING_KID="$DEV_SIGNING_KID" \
  UGOITE_DEV_SIGNING_SECRET="$DEV_SIGNING_SECRET" \
  UGOITE_AUTH_BEARER_SECRETS="$DEV_SIGNING_KID:$DEV_SIGNING_SECRET" \
  UGOITE_AUTH_BEARER_ACTIVE_KIDS="$DEV_SIGNING_KID" \
  UGOITE_AUTH_BEARER_TOKENS_JSON="$STATIC_E2E_TOKENS_JSON" \
  uv run uvicorn src.app.main:app --host 0.0.0.0 --port 8000 &
BACKEND_PID=$!

echo "Starting frontend server..."
cd "$ROOT_DIR/frontend"
FRONTEND_MODE="${E2E_FRONTEND_MODE:-dev}"
if [ "$FRONTEND_MODE" = "prod" ]; then
  echo "Building frontend for production..."
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" bun run build
  echo "Starting production frontend server..."
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" NODE_ENV=production bun run start &
else
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" bun run dev &
fi
FRONTEND_PID=$!

cleanup() {
  echo ""
  echo "Stopping servers..."
  if [ -n "${BACKEND_PID:-}" ]; then
    kill "$BACKEND_PID" 2>/dev/null || true
  fi
  if [ -n "${FRONTEND_PID:-}" ]; then
    kill "$FRONTEND_PID" 2>/dev/null || true
  fi
  wait "${BACKEND_PID:-}" "${FRONTEND_PID:-}" 2>/dev/null || true
  echo "Servers stopped."
  if [ "$CLEANUP_E2E_STORAGE" = true ]; then
    rm -rf "$E2E_STORAGE_ROOT"
  fi
}
trap cleanup EXIT INT TERM

echo "Waiting for backend (port 8000)..."
for i in {1..30}; do
  if curl -s http://localhost:8000/health >/dev/null 2>&1; then
    echo "✓ Backend is ready!"
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "✗ ERROR: Backend failed to start within 30 seconds"
    exit 1
  fi
  sleep 1
done

E2E_AUTH_BEARER_TOKEN="$(
  curl -fsS -X POST http://localhost:8000/auth/mock-oauth | python -c 'import json, sys; print(json.load(sys.stdin)["bearer_token"])'
)"
export E2E_AUTH_BEARER_TOKEN

echo "Waiting for frontend (port 3000)..."
for i in {1..60}; do
  if curl -s http://localhost:3000 >/dev/null 2>&1; then
    echo "✓ Frontend is ready!"
    break
  fi
  if [ "$i" -eq 60 ]; then
    echo "✗ ERROR: Frontend failed to start within 60 seconds"
    exit 1
  fi
  sleep 1
done

echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"

if [ "$ENFORCE_CI_GATES" = "true" ]; then
  mkdir -p "$(dirname "$PLAYWRIGHT_JUNIT_OUTPUT_FILE")"
  rm -f "$PLAYWRIGHT_JUNIT_OUTPUT_FILE"
fi

TEST_TIMEOUT_ARGS=()
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
  TEST_TIMEOUT_ARGS=(--timeout "${E2E_TEST_TIMEOUT_MS}")
fi

case "$TEST_TYPE" in
  smoke)
    npm run test:smoke -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  entries)
    npm run test:entries -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  screenshot)
    npm run test:screenshot -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  full)
    npm run test -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./e2e/scripts/run-e2e.sh [smoke|entries|screenshot|full]"
    exit 1
    ;;
esac

if [ "$ENFORCE_CI_GATES" = "true" ]; then
  python3 - <<'PY'
import os
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

report = Path(os.environ["PLAYWRIGHT_JUNIT_OUTPUT_FILE"])
if not report.exists():
    raise SystemExit(f"missing junit report: {report}")

root = ET.parse(report).getroot()
suites = [root] if root.tag == "testsuite" else list(root.findall("testsuite"))
skipped = sum(int(float(s.attrib.get("skipped", "0") or 0)) for s in suites)
tests = sum(int(float(s.attrib.get("tests", "0") or 0)) for s in suites)
if tests == 0:
    raise SystemExit("e2e tests: zero executed tests")
if skipped > 0:
    raise SystemExit(f"e2e tests: skipped={skipped} is not allowed")
sys.stdout.write(f"e2e tests OK: tests={tests}, skipped={skipped}\n")
PY
fi

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
