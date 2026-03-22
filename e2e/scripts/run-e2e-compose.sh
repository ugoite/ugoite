#!/bin/bash
# E2E test runner using Docker Compose with locally built or pre-built images.
# Used by local `mise run e2e` and by GitHub Actions e2e-ci.yml.
#
# Usage: ./e2e/scripts/run-e2e-compose.sh [test-type]
#   test-type: "smoke", "entries", "screenshot", or "full" (default)
#
# Environment variables:
#   E2E_BUILD_IMAGES: "true" (default) to build local images before startup;
#     "false" to reuse pre-built images (used in CI)
#   E2E_BACKEND_START_TIMEOUT_SECONDS / E2E_FRONTEND_START_TIMEOUT_SECONDS:
#     optional startup wait budgets for compose services (defaults: 120 seconds)
#   E2E_TEST_TIMEOUT_MS: optional per-test timeout passed to Playwright

set -e

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker-compose.e2e.yml"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-e2e-local-signing-secret}"
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
BUILD_IMAGES="${E2E_BUILD_IMAGES:-true}"
STATIC_E2E_TOKENS_JSON='{"alice-token":{"user_id":"alice-user","principal_type":"user"},"bob-token":{"user_id":"bob-user","principal_type":"user"}}'
DEV_AUTH_PROXY_TOKEN="${UGOITE_DEV_AUTH_PROXY_TOKEN:-e2e-dev-auth-proxy-token}"
export UGOITE_DEV_AUTH_MODE=mock-oauth
export UGOITE_DEV_USER_ID=e2e-user
export UGOITE_DEV_SIGNING_KID="$DEV_SIGNING_KID"
export UGOITE_DEV_SIGNING_SECRET="$DEV_SIGNING_SECRET"
export UGOITE_DEV_AUTH_PROXY_TOKEN="$DEV_AUTH_PROXY_TOKEN"
export UGOITE_AUTH_BEARER_SECRETS="$DEV_SIGNING_KID:$DEV_SIGNING_SECRET"
export UGOITE_AUTH_BEARER_ACTIVE_KIDS="$DEV_SIGNING_KID"
export UGOITE_AUTH_BEARER_TOKENS_JSON="$STATIC_E2E_TOKENS_JSON"
export UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS"

backend_start_timeout="${E2E_BACKEND_START_TIMEOUT_SECONDS:-120}"
frontend_start_timeout="${E2E_FRONTEND_START_TIMEOUT_SECONDS:-120}"
export PLAYWRIGHT_CI_REPORTER=junit
export PLAYWRIGHT_JUNIT_OUTPUT_FILE="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"

compose_cmd=(docker compose -f "$COMPOSE_FILE")

cleanup() {
  echo ""
  echo "Stopping services..."
  "${compose_cmd[@]}" down -v 2>/dev/null || true
  echo "Services stopped."
}
trap cleanup EXIT INT TERM

if [ "$BUILD_IMAGES" = "true" ]; then
  echo "Building services via docker-compose.e2e.yml..."
  "${compose_cmd[@]}" build
fi

echo "Starting services via docker-compose.e2e.yml..."
"${compose_cmd[@]}" up -d

echo "Waiting for backend (port 8000)..."
for i in $(seq 1 "$backend_start_timeout"); do
  if curl -sf "http://localhost:8000/health" >/dev/null 2>&1; then
    echo "✓ Backend is ready!"
    break
  fi
  if [ "$i" -eq "$backend_start_timeout" ]; then
    echo "✗ ERROR: Backend failed to start within ${backend_start_timeout} seconds"
    "${compose_cmd[@]}" logs backend
    exit 1
  fi
  sleep 1
done

E2E_AUTH_BEARER_TOKEN="$(
  "${compose_cmd[@]}" exec -T backend python -c '
import json
from urllib.request import Request, urlopen

request = Request("http://127.0.0.1:8000/auth/mock-oauth", method="POST")
with urlopen(request) as response:
    print(json.load(response)["bearer_token"])
'
)"
export E2E_AUTH_BEARER_TOKEN

echo "Waiting for frontend (port 3000)..."
for i in $(seq 1 "$frontend_start_timeout"); do
  if curl -sf "http://localhost:3000" >/dev/null 2>&1; then
    echo "✓ Frontend is ready!"
    break
  fi
  if [ "$i" -eq "$frontend_start_timeout" ]; then
    echo "✗ ERROR: Frontend failed to start within ${frontend_start_timeout} seconds"
    "${compose_cmd[@]}" logs frontend
    exit 1
  fi
  sleep 1
done

echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"
mkdir -p "$(dirname "$PLAYWRIGHT_JUNIT_OUTPUT_FILE")"
rm -f "$PLAYWRIGHT_JUNIT_OUTPUT_FILE"
case "$TEST_TYPE" in
  smoke)
    cmd=(npm run test:smoke --)
    ;;
  entries)
    cmd=(npm run test:entries --)
    ;;
  screenshot)
    cmd=(npm run test:screenshot --)
    ;;
  full)
    cmd=(npm run test --)
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./e2e/scripts/run-e2e-compose.sh [smoke|entries|screenshot|full]"
    exit 1
    ;;
esac
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
  cmd+=(--timeout "$E2E_TEST_TIMEOUT_MS")
fi
"${cmd[@]}"

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

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
