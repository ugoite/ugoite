#!/bin/bash
# E2E test runner using Docker Compose with locally built or pre-built images.
# Used by local `mise run e2e` and by GitHub Actions e2e-ci.yml.
#
# Usage: ./e2e/scripts/run-e2e-compose.sh [test-type]
#   test-type: "smoke", "asset-owned", "smoke-and-asset-owned",
#     "owner-recovery", "entries", "screenshot", or "full" (default)
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
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
BUILD_IMAGES="${E2E_BUILD_IMAGES:-true}"
export UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS"
export E2E_COMPOSE_PORT="${E2E_COMPOSE_PORT:-18000}"
export UGOITE_PUBLIC_ORIGIN="http://localhost:${E2E_COMPOSE_PORT}"
export UGOITE_API_BASE_URL="${UGOITE_PUBLIC_ORIGIN}/api"
export UGOITE_WEBAUTHN_RP_ID="localhost"
export UGOITE_NODE_SECRET_KEY="${UGOITE_NODE_SECRET_KEY:-$(head -c 32 /dev/urandom | base64)}"
export FRONTEND_URL="$UGOITE_PUBLIC_ORIGIN"
export BACKEND_URL="$UGOITE_PUBLIC_ORIGIN"

detect_host_address() {
  if command -v ipconfig >/dev/null 2>&1; then
    for interface in en0 en1; do
      address="$(ipconfig getifaddr "$interface" 2>/dev/null || true)"
      case "$address" in
        127.*) ;;
        *.*)
          echo "$address"
          return 0
          ;;
      esac
    done
  fi
  if command -v ip >/dev/null 2>&1; then
    address="$(ip route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([^ ]*\).*/\1/p' | head -n 1)"
    case "$address" in
      127.*) ;;
      *.*)
        echo "$address"
        return 0
        ;;
    esac
  fi
  if command -v hostname >/dev/null 2>&1; then
    address="$(hostname -I 2>/dev/null | awk '{print $1}')"
    case "$address" in
      127.*) ;;
      *.*)
        echo "$address"
        return 0
        ;;
    esac
  fi
  return 1
}

is_container_reachable_host() {
  case "$1" in
    ""|localhost|127.*|0.0.0.0|::1) return 1 ;;
    *[!A-Za-z0-9._-]*) return 1 ;;
    *) return 0 ;;
  esac
}

resolve_oidc_mock_host() {
  if [ -n "${E2E_OIDC_MOCK_HOST:-}" ]; then
    if ! is_container_reachable_host "$E2E_OIDC_MOCK_HOST"; then
      echo "✗ ERROR: E2E_OIDC_MOCK_HOST must be a non-loopback host name or IPv4 address reachable from the Compose container" >&2
      return 1
    fi
    printf '%s\n' "$E2E_OIDC_MOCK_HOST"
    return 0
  fi

  if ! address="$(detect_host_address)"; then
    echo "✗ ERROR: could not determine a non-loopback host address for the Compose OIDC mock" >&2
    echo "  Set E2E_OIDC_MOCK_HOST to a host name or IPv4 address reachable from the Compose container" >&2
    return 1
  fi
  printf '%s\n' "$address"
}

# The browser runs on the host while the composed backend runs in a container.
# Advertise a host address that both sides can reach; direct-process E2E keeps
# the mock on loopback by leaving this unset.
if ! resolved_oidc_mock_host="$(resolve_oidc_mock_host)"; then
  exit 1
fi
export E2E_OIDC_MOCK_HOST="$resolved_oidc_mock_host"

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

compose_host_port="$("${compose_cmd[@]}" port ugoite 8000 | sed -E 's/.*:([0-9]+)$/\1/')"
if [ -z "$compose_host_port" ]; then
  echo "✗ ERROR: could not determine the published compose port"
  "${compose_cmd[@]}" logs ugoite
  exit 1
fi

export FRONTEND_URL="http://localhost:${compose_host_port}"
export BACKEND_URL="http://localhost:${compose_host_port}"

echo "Published compose port: ${compose_host_port}"
echo "Waiting for backend (${BACKEND_URL})..."
for i in $(seq 1 "$backend_start_timeout"); do
  if curl -sf "${BACKEND_URL%/}/health" >/dev/null 2>&1; then
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

E2E_SETUP_SECRET="$("${compose_cmd[@]}" logs --no-color ugoite | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | tail -n 1)"
if [ -z "$E2E_SETUP_SECRET" ]; then
  echo "✗ ERROR: setup secret was not present in the container startup log"
  exit 1
fi
export E2E_SETUP_SECRET

echo "Frontend URL: $FRONTEND_URL"

echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"
base_report_file="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"

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

run_e2e_task() {
  local task="$1"
  local report="$2"
  export PLAYWRIGHT_JUNIT_OUTPUT_FILE="$report"
  mkdir -p "$(dirname "$report")"
  rm -f "$report"

  cmd=(deno task "$task" --)
  if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
    cmd+=(--timeout "$E2E_TEST_TIMEOUT_MS")
  fi
  "${cmd[@]}"
  validate_junit_report "$report"
}

case "$TEST_TYPE" in
  smoke)
    run_e2e_task smoke "$base_report_file"
    ;;
  asset-owned)
    run_e2e_task asset-owned "$base_report_file"
    ;;
  smoke-and-asset-owned)
    run_e2e_task smoke-and-asset-owned "$base_report_file"
    ;;
  owner-recovery)
    run_e2e_task owner-recovery "$base_report_file"
    ;;
  entries)
    run_e2e_task entries "$base_report_file"
    ;;
  screenshot)
    run_e2e_task screenshot "$base_report_file"
    ;;
  full)
    run_e2e_task full "$base_report_file"
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./run-e2e-compose.sh [smoke|asset-owned|smoke-and-asset-owned|owner-recovery|entries|screenshot|full]"
    exit 1
    ;;
esac

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
