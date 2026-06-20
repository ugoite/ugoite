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
#   E2E_BACKEND_START_TIMEOUT_SECONDS:
#     optional startup wait budget for the composed service (default: 120 seconds)
#   E2E_TEST_TIMEOUT_MS: optional per-test timeout passed to Playwright

set -e

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker-compose.e2e.yml"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-e2e-local-signing-secret-0123456789abcdef}"
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
BUILD_IMAGES="${E2E_BUILD_IMAGES:-true}"
STATIC_E2E_TOKENS_JSON='{"e2e-token":{"user_id":"e2e-user","principal_type":"user"},"alice-token":{"user_id":"alice-user","principal_type":"user"},"bob-token":{"user_id":"bob-user","principal_type":"user"}}'
export UGOITE_DEV_AUTH_MODE=mock-oauth
export UGOITE_DEV_USER_ID=e2e-user
export UGOITE_DEV_SIGNING_KID="$DEV_SIGNING_KID"
export UGOITE_DEV_SIGNING_SECRET="$DEV_SIGNING_SECRET"
export UGOITE_BOOTSTRAP_TOKEN=e2e-token
export UGOITE_AUTH_BEARER_TOKENS="$STATIC_E2E_TOKENS_JSON"
export UGOITE_AUTH_BEARER_SIGNING_SECRETS="$DEV_SIGNING_KID:$DEV_SIGNING_SECRET"
export UGOITE_AUTH_BEARER_ACTIVE_KIDS="$DEV_SIGNING_KID"
export UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS"
export FRONTEND_URL="${FRONTEND_URL:-http://localhost:8000}"
export BACKEND_URL="${BACKEND_URL:-http://localhost:8000}"

ensure_playwright_browsers() {
  if [ "${UGOITE_SKIP_PLAYWRIGHT_DEPS:-}" = "1" ]; then
    echo "Skipping Playwright browser install because UGOITE_SKIP_PLAYWRIGHT_DEPS=1"
    return
  fi
  echo "Installing Playwright browsers..."
  (cd "$ROOT_DIR/e2e" && deno task install:browsers)
}

ensure_playwright_browsers

backend_start_timeout="${E2E_BACKEND_START_TIMEOUT_SECONDS:-120}"
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
    "${compose_cmd[@]}" logs ugoite
    exit 1
  fi
  sleep 1
done

E2E_AUTH_BEARER_TOKEN="e2e-token"
export E2E_AUTH_BEARER_TOKEN

echo "Frontend URL: $FRONTEND_URL"

echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"
mkdir -p "$(dirname "$PLAYWRIGHT_JUNIT_OUTPUT_FILE")"
rm -f "$PLAYWRIGHT_JUNIT_OUTPUT_FILE"
case "$TEST_TYPE" in
  smoke)
    cmd=(deno task smoke --)
    ;;
  entries)
    cmd=(deno task entries --)
    ;;
  screenshot)
    cmd=(deno task screenshot --)
    ;;
  full)
    cmd=(deno task full --)
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

deno eval '
  const report = Deno.env.get("PLAYWRIGHT_JUNIT_OUTPUT_FILE");
  if (!report) throw new Error("PLAYWRIGHT_JUNIT_OUTPUT_FILE is required");
  const xml = await Deno.readTextFile(report);
  const suites = [...xml.matchAll(/<testsuite\b[^>]*>/g)].map((match) => match[0]);
  const attr = (text, name) => Number(text.match(new RegExp(`${name}="([^"]*)"`))?.[1] ?? 0);
  const tests = suites.reduce((sum, suite) => sum + attr(suite, "tests"), 0);
  const skipped = suites.reduce((sum, suite) => sum + attr(suite, "skipped"), 0);
  if (tests === 0) throw new Error("e2e tests: zero executed tests");
  if (skipped > 0) throw new Error(`e2e tests: skipped=${skipped} is not allowed`);
  console.log(`e2e tests OK: tests=${tests}, skipped=${skipped}`);
'

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
