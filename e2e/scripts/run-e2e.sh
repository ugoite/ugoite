#!/bin/bash
# Direct-process E2E runner for fast local iteration and for no-Docker parity
# fallback via `run-e2e-parity.sh`.
#
# Usage: ./e2e/scripts/run-e2e.sh [test-type]
#   test-type: "smoke", "asset-owned", "smoke-and-asset-owned", "entries",
#     "screenshot", or "full" (runs standard tests)
#
# Environment variables:
#   E2E_TEST_TIMEOUT_MS: per-test timeout passed to `playwright test --timeout`
#   E2E_FRONTEND_MODE: "dev" (default), "prod" to use build+start for SSR speed,
#     or "static" to serve the built SPA from ugoite-server
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
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
ENFORCE_CI_GATES="${E2E_ENFORCE_CI_GATES:-false}"
FRONTEND_MODE="${E2E_FRONTEND_MODE:-static}"
if [ "$FRONTEND_MODE" = "static" ]; then
  FRONTEND_URL="${FRONTEND_URL:-http://localhost:8000}"
else
  FRONTEND_URL="${FRONTEND_URL:-http://localhost:3000}"
fi
BACKEND_URL="${BACKEND_URL:-http://localhost:8000}"
export FRONTEND_URL
export BACKEND_URL

url_port() {
  local url="$1"
  local without_path="${url#*://}"
  local host_port="${without_path%%/*}"
  local port="${host_port##*:}"
  if [ "$port" = "$host_port" ]; then
    case "$url" in
      http://*) echo "80" ;;
      https://*) echo "443" ;;
      *) echo "" ;;
    esac
  else
    echo "$port"
  fi
}

FRONTEND_PORT="$(url_port "$FRONTEND_URL")"
BACKEND_PORT="$(url_port "$BACKEND_URL")"

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
  if command -v lsof >/dev/null 2>&1 && lsof -nP -iTCP:"${port}" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "✗ ERROR: ${label} port ${port} is already in use."
    describe_port "$port"
    echo "Stop the conflicting process, or set FRONTEND_URL/BACKEND_URL to free ports before running E2E."
    exit 1
  fi
  if command -v fuser >/dev/null 2>&1 && fuser "${port}/tcp" >/dev/null 2>&1; then
    echo "✗ ERROR: ${label} port ${port} is already in use."
    describe_port "$port"
    echo "Stop the conflicting process, or set FRONTEND_URL/BACKEND_URL to free ports before running E2E."
    exit 1
  fi
}

echo "Checking that ports ${BACKEND_PORT} and ${FRONTEND_PORT} are free..."
ensure_port_available "$BACKEND_PORT" "Backend"
ensure_port_available "$FRONTEND_PORT" "Frontend"

E2E_STORAGE_ROOT="${E2E_STORAGE_ROOT:-}"
if [ -z "$E2E_STORAGE_ROOT" ]; then
  E2E_STORAGE_ROOT="/tmp/ugoite-e2e"
  CLEANUP_E2E_STORAGE=true
else
  CLEANUP_E2E_STORAGE=false
fi

mkdir -p "$E2E_STORAGE_ROOT"

ensure_playwright_browsers() {
  if [ "${UGOITE_SKIP_PLAYWRIGHT_DEPS:-}" = "1" ]; then
    echo "Skipping Playwright browser install because UGOITE_SKIP_PLAYWRIGHT_DEPS=1"
    return
  fi
  echo "Installing Playwright browsers..."
  (cd "$ROOT_DIR/e2e" && deno task install:browsers)
}

ensure_playwright_browsers

STATIC_DIR=""
if [ "$FRONTEND_MODE" = "static" ]; then
  echo "Building static frontend..."
  cd "$ROOT_DIR"
  UGOITE_STATIC_SPA=true deno task frontend:build
  deno run -A frontend/scripts/generate-static-index.ts \
    frontend/.output/public/_build/.vite/manifest.json \
    frontend/.output/public/index.html
  STATIC_DIR="$ROOT_DIR/frontend/.output/public"
fi

echo "Starting backend server..."
cd "$ROOT_DIR"
BACKEND_ENV=(
  "UGOITE_ROOT=$E2E_STORAGE_ROOT"
  "UGOITE_SERVER_ADDRESS=0.0.0.0:$BACKEND_PORT"
  "UGOITE_PUBLIC_ORIGIN=$FRONTEND_URL"
  "UGOITE_E2E_TEST_MODE=true"
  "UGOITE_WEBAUTHN_RP_ID=$(node -e 'console.log(new URL(process.argv[1]).hostname)' "$FRONTEND_URL")"
  "UGOITE_API_BASE_URL=$FRONTEND_URL/api"
  "UGOITE_NODE_SECRET_KEY=${UGOITE_NODE_SECRET_KEY:-$(head -c 32 /dev/urandom | base64)}"
)
if [ -n "$STATIC_DIR" ]; then
  BACKEND_ENV+=("UGOITE_STATIC_DIR=$STATIC_DIR")
fi
BACKEND_LOG="$E2E_STORAGE_ROOT/backend.log"
env "${BACKEND_ENV[@]}" cargo run -p ugoite-server --locked > >(tee "$BACKEND_LOG") 2>&1 &
BACKEND_PID=$!

FRONTEND_PID=""
if [ "$FRONTEND_MODE" != "static" ]; then
  echo "Starting frontend server..."
  cd "$ROOT_DIR/frontend"
  if [ "$FRONTEND_MODE" = "prod" ]; then
    echo "Building frontend for production..."
    BACKEND_URL="$BACKEND_URL" VITE_API_PROXY=true UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" deno task build
    echo "Starting production frontend server..."
    BACKEND_URL="$BACKEND_URL" VITE_API_PROXY=true UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" NODE_ENV=production PORT="$FRONTEND_PORT" deno task start &
  else
    BACKEND_URL="$BACKEND_URL" VITE_API_PROXY=true UGOITE_STATIC_SPA=true UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS" PORT="$FRONTEND_PORT" deno task dev &
  fi
  FRONTEND_PID=$!
fi

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

echo "Waiting for backend (${BACKEND_URL})..."
for i in {1..30}; do
  if curl -s "${BACKEND_URL%/}/health" >/dev/null 2>&1; then
    echo "✓ Backend is ready!"
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "✗ ERROR: Backend failed to start within 30 seconds"
    exit 1
  fi
  sleep 1
done

E2E_SETUP_SECRET="$(sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' "$BACKEND_LOG" | tail -n 1)"
if [ -z "$E2E_SETUP_SECRET" ]; then
  echo "✗ ERROR: setup secret was not present in the local startup log"
  exit 1
fi
export E2E_SETUP_SECRET

echo "Waiting for frontend (${FRONTEND_URL})..."
for i in {1..60}; do
  if curl -s "$FRONTEND_URL" >/dev/null 2>&1; then
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
base_report_file="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"

TEST_TIMEOUT_ARGS=()
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
  TEST_TIMEOUT_ARGS=(--timeout "${E2E_TEST_TIMEOUT_MS}")
fi
run_e2e_task() {
  local task="$1"
  local report="$2"
  if [ "$ENFORCE_CI_GATES" = "true" ]; then
    export PLAYWRIGHT_JUNIT_OUTPUT_FILE="$report"
    mkdir -p "$(dirname "$report")"
    rm -f "$report"
  fi
  if [ "${#TEST_TIMEOUT_ARGS[@]}" -gt 0 ]; then
    deno task "$task" -- "${TEST_TIMEOUT_ARGS[@]}"
  else
    deno task "$task"
  fi
  if [ "$ENFORCE_CI_GATES" = "true" ]; then
    validate_junit_report "$report"
  fi
}

validate_junit_report() {
  local report="$1"
  PLAYWRIGHT_JUNIT_OUTPUT_FILE="$report" deno eval '
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
}

case "$TEST_TYPE" in
  smoke)
    run_e2e_task smoke "$base_report_file"
    ;;
  entries)
    run_e2e_task entries "$base_report_file"
    ;;
  asset-owned)
    run_e2e_task asset-owned "$base_report_file"
    ;;
  smoke-and-asset-owned)
    run_e2e_task smoke-and-asset-owned "$base_report_file"
    ;;
  screenshot)
    run_e2e_task screenshot "$base_report_file"
    ;;
  full)
    run_e2e_task full "$base_report_file"
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./e2e/scripts/run-e2e.sh [smoke|asset-owned|smoke-and-asset-owned|entries|screenshot|full]"
    exit 1
    ;;
esac

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
