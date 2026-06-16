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

# Unset VIRTUAL_ENV to avoid inheriting an unrelated Python environment from
# the current shell session while the runner uses Cargo and Deno.
unset VIRTUAL_ENV
export BASELINE_BROWSER_MAPPING_IGNORE_OLD_DATA=true
export BROWSERSLIST_IGNORE_OLD_DATA=true

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-e2e-local-signing-secret-0123456789abcdef}"
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
STATIC_E2E_TOKENS_JSON='{"e2e-token":{"user_id":"e2e-user","principal_type":"user"},"alice-token":{"user_id":"alice-user","principal_type":"user"},"bob-token":{"user_id":"bob-user","principal_type":"user"}}'
ENFORCE_CI_GATES="${E2E_ENFORCE_CI_GATES:-false}"

if [ "$ENFORCE_CI_GATES" = "true" ]; then
  export PLAYWRIGHT_CI_REPORTER=junit
  export PLAYWRIGHT_JUNIT_OUTPUT_FILE="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"
fi

describe_port() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"${port}" -sTCP:LISTEN || true
  else
    ss -ltnp "( sport = :${port} )" || true
  fi
}

ensure_port_available() {
  local port="$1"
  local label="$2"
  if fuser "${port}/tcp" >/dev/null 2>&1; then
    echo "✗ ERROR: ${label} port ${port} is already in use."
    describe_port "$port"
    echo "Stop the conflicting process, or run \`mise run cleanup:ports\` if you want to clear standard Ugoite dev ports explicitly."
    exit 1
  fi
}

echo "Checking that ports 8000 and 3000 are free..."
ensure_port_available 8000 "Backend"
ensure_port_available 3000 "Frontend"

E2E_STORAGE_ROOT="${E2E_STORAGE_ROOT:-}"
if [ -z "$E2E_STORAGE_ROOT" ]; then
  E2E_STORAGE_ROOT="/tmp/ugoite-e2e"
  CLEANUP_E2E_STORAGE=true
else
  CLEANUP_E2E_STORAGE=false
fi

mkdir -p "$E2E_STORAGE_ROOT"

echo "Starting backend server..."
cd "$ROOT_DIR"
UGOITE_ROOT="$E2E_STORAGE_ROOT" \
  UGOITE_SERVER_ADDRESS=0.0.0.0:8000 \
  UGOITE_BOOTSTRAP_DEFAULT_SPACE=true \
  UGOITE_BOOTSTRAP_TOKEN=e2e-token \
  UGOITE_DEV_AUTH_MODE=mock-oauth \
  UGOITE_DEV_USER_ID=e2e-user \
  UGOITE_DEV_SIGNING_KID="$DEV_SIGNING_KID" \
  UGOITE_DEV_SIGNING_SECRET="$DEV_SIGNING_SECRET" \
  UGOITE_AUTH_BEARER_TOKENS="$STATIC_E2E_TOKENS_JSON" \
  UGOITE_AUTH_BEARER_SIGNING_SECRETS="$DEV_SIGNING_KID:$DEV_SIGNING_SECRET" \
  UGOITE_AUTH_BEARER_ACTIVE_KIDS="$DEV_SIGNING_KID" \
  cargo run -p ugoite-server --locked &
BACKEND_PID=$!

echo "Starting frontend server..."
cd "$ROOT_DIR/frontend"
FRONTEND_MODE="${E2E_FRONTEND_MODE:-dev}"
if [ "$FRONTEND_MODE" = "prod" ]; then
  echo "Building frontend for production..."
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" deno task build
  echo "Starting production frontend server..."
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" NODE_ENV=production deno task start &
else
  BACKEND_URL=http://localhost:8000 UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" deno task dev &
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

E2E_AUTH_BEARER_TOKEN="e2e-token"
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
    deno task smoke -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  entries)
    deno task entries -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  screenshot)
    deno task screenshot -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  full)
    deno task full -- "${TEST_TIMEOUT_ARGS[@]+"${TEST_TIMEOUT_ARGS[@]}"}"
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./e2e/scripts/run-e2e.sh [smoke|entries|screenshot|full]"
    exit 1
    ;;
esac

if [ "$ENFORCE_CI_GATES" = "true" ]; then
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
fi

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
