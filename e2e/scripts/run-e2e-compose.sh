#!/bin/bash
# E2E test runner using Docker Compose with locally built or pre-built images.
# Used by local `mise run e2e` and by GitHub Actions e2e-ci.yml.
#
# Usage: ./e2e/scripts/run-e2e-compose.sh [test-type]
#   test-type: "smoke", "asset-owned", "smoke-and-asset-owned",
#     "owner-recovery", "mobile-ui", "qry02", "entries", "screenshot", or "full"
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
CHECKOUT_SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [ -n "${UGOITE_SOURCE_SHA:-}" ] && [ "$UGOITE_SOURCE_SHA" != "$CHECKOUT_SOURCE_SHA" ]; then
  echo "✗ ERROR: UGOITE_SOURCE_SHA does not match the checkout under test"
  echo "  expected: $CHECKOUT_SOURCE_SHA"
  echo "  received: $UGOITE_SOURCE_SHA"
  exit 1
fi
export UGOITE_SOURCE_SHA="$CHECKOUT_SOURCE_SHA"
PROXY_TIMEOUT_MS="${UGOITE_PROXY_TIMEOUT_MS:-30000}"
BUILD_IMAGES="${E2E_BUILD_IMAGES:-true}"
export UGOITE_PROXY_TIMEOUT_MS="$PROXY_TIMEOUT_MS"

free_port() {
  deno eval 'const listener = Deno.listen({ hostname: "127.0.0.1", port: 0 }); console.log((listener.addr as Deno.NetAddr).port); listener.close();'
}

export E2E_COMPOSE_PORT="${E2E_COMPOSE_PORT:-$(free_port)}"
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

# Never clobber a pre-existing dev provenance file: back it up around any dev
# E2E generation and restore it on exit (mirrors run-e2e.sh dev handling).
DEV_BUILD_INFO_PATH="$ROOT_DIR/frontend/public/build-info.json"
DEV_BUILD_INFO_BACKUP=""
if [ -f "$DEV_BUILD_INFO_PATH" ]; then
  DEV_BUILD_INFO_BACKUP="$(mktemp)"
  cp "$DEV_BUILD_INFO_PATH" "$DEV_BUILD_INFO_BACKUP"
fi

backend_start_timeout="${E2E_BACKEND_START_TIMEOUT_SECONDS:-120}"
export PLAYWRIGHT_CI_REPORTER=junit
export PLAYWRIGHT_JUNIT_OUTPUT_FILE="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/junit.xml}"

compose_cmd=(docker compose -f "$COMPOSE_FILE")

cleanup() {
  echo ""
  echo "Stopping services..."
  "${compose_cmd[@]}" down -v 2>/dev/null || true
  if [ -n "${DEV_BUILD_INFO_BACKUP:-}" ] && [ -f "$DEV_BUILD_INFO_BACKUP" ]; then
    mv "$DEV_BUILD_INFO_BACKUP" "$DEV_BUILD_INFO_PATH"
  fi
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

# PR-01 product readiness gate (issue #2910): /health 200 alone does not prove
# the composed image serves THIS checkout. Poll each product signal
# sequentially before Playwright starts so a blank /setup page fails here
# with diagnostics instead of flaking inside the browser run.
readiness_timeout="${E2E_READINESS_TIMEOUT_SECONDS:-60}"
EXPECTED_SOURCE_SHA="$CHECKOUT_SOURCE_SHA"

readiness_diagnostics() {
  local phase="$1"
  local url="$2"
  local reason="$3"
  echo "✗ ERROR: product readiness failed at phase: ${phase}"
  echo "  failed request URL: ${url}"
  echo "  reason: ${reason}"
  echo "  expected source SHA: ${EXPECTED_SOURCE_SHA}"
  local actual_header_sha
  actual_header_sha="$(curl -sSI "${BACKEND_URL%/}/health" 2>/dev/null | grep -i '^x-ugoite-source-sha:' | tr -d '\r' | awk '{print $2}')"
  echo "  actual X-Ugoite-Source-Sha header: ${actual_header_sha:-<unavailable>}"
  local actual_build_info
  actual_build_info="$(curl -s "${BACKEND_URL%/}/build-info.json" 2>/dev/null | head -c 500)"
  echo "  actual /build-info.json snippet: ${actual_build_info:-<unavailable>}"
  local setup_snippet
  setup_snippet="$(curl -s "${BACKEND_URL%/}/setup" 2>/dev/null | head -c 500)"
  echo "  actual /setup body snippet: ${setup_snippet:-<unavailable>}"
  local setup_code
  setup_code="$(curl -s -o /dev/null -w '%{http_code}' "${BACKEND_URL%/}/setup" 2>/dev/null)"
  echo "  actual /setup HTTP status: ${setup_code:-<unavailable>}"
  echo "  container log tail (ugoite, last 100 lines):"
  "${compose_cmd[@]}" logs --no-color --tail=100 ugoite 2>/dev/null || true
  exit 1
}

echo "Checking product readiness (expected SHA: ${EXPECTED_SOURCE_SHA})..."

echo "  [1/4] /health source SHA header..."
health_sha=""
for i in $(seq 1 "$readiness_timeout"); do
  if curl -sf "${BACKEND_URL%/}/health" >/dev/null 2>&1; then
    health_sha="$(curl -sSI "${BACKEND_URL%/}/health" 2>/dev/null | grep -i '^x-ugoite-source-sha:' | tr -d '\r' | awk '{print $2}')"
    if [ "$health_sha" = "$EXPECTED_SOURCE_SHA" ]; then
      echo "  ✓ /health serves expected SHA"
      break
    fi
  fi
  if [ "$i" -eq "$readiness_timeout" ]; then
    readiness_diagnostics "health-sha" "${BACKEND_URL%/}/health" "X-Ugoite-Source-Sha header did not match checkout SHA within ${readiness_timeout}s (last seen: ${health_sha:-<none>})"
  fi
  sleep 1
done

echo "  [2/4] /build-info.json source_sha..."
build_info_sha=""
for i in $(seq 1 "$readiness_timeout"); do
  build_info_body="$(curl -s "${BACKEND_URL%/}/build-info.json" 2>/dev/null || true)"
  if [ -n "$build_info_body" ]; then
    build_info_sha="$(printf '%s' "$build_info_body" | grep -o '"source_sha"[[:space:]]*:[[:space:]]*"[^"]*"' | head -n 1 | sed -E 's/.*\"([0-9a-f]{40}|unknown)\".*/\1/')"
    if [ "$build_info_sha" = "$EXPECTED_SOURCE_SHA" ]; then
      echo "  ✓ /build-info.json serves expected SHA"
      break
    fi
  fi
  if [ "$i" -eq "$readiness_timeout" ]; then
    readiness_diagnostics "build-info-sha" "${BACKEND_URL%/}/build-info.json" "/build-info.json source_sha did not match checkout SHA within ${readiness_timeout}s (last seen: ${build_info_sha:-<none>})"
  fi
  sleep 1
done

# The shipped frontend entrypoint always renders <div id="app"> plus a built
# asset reference (/_build/ script + ugoite-manifest.js). Sources:
# frontend/scripts/generate-static-index.ts, frontend/src/entry-server.tsx,
# frontend/src/runtime/start-server.ts. A 2xx /setup without these is the
# blank-page bootstrap failure from #2910, not a ready product.
echo "  [3/4] /setup serves shipped frontend entrypoint..."
for i in $(seq 1 "$readiness_timeout"); do
  setup_body="$(curl -s "${BACKEND_URL%/}/setup" 2>/dev/null || true)"
  setup_status="$(curl -s -o /dev/null -w '%{http_code}' "${BACKEND_URL%/}/setup" 2>/dev/null || true)"
  if [ -n "$setup_body" ] \
    && printf '%s' "$setup_body" | grep -q '<div id="app"' \
    && printf '%s' "$setup_body" | grep -q '/_build/'; then
    echo "  ✓ /setup serves shipped frontend entrypoint (HTTP ${setup_status})"
    break
  fi
  if [ "$i" -eq "$readiness_timeout" ]; then
    readiness_diagnostics "setup-entrypoint" "${BACKEND_URL%/}/setup" "/setup (HTTP ${setup_status:-<unknown>}) lacked the shipped frontend entrypoint (<div id=\"app\" + /_build/ asset) within ${readiness_timeout}s"
  fi
  sleep 1
done

echo "  [4/4] setup secret uniquely extractable from container log..."
setup_log="$("${compose_cmd[@]}" logs --no-color ugoite 2>/dev/null || true)"
secret_count="$(printf '%s' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | wc -l | tr -d ' ')"
distinct_secret_count="$(printf '%s' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | sort -u | wc -l | tr -d ' ')"
if [ -z "$secret_count" ] || [ "$secret_count" -eq 0 ]; then
  echo "✗ ERROR: setup secret was not present in the container startup log"
  echo "  container log tail (ugoite, last 100 lines):"
  "${compose_cmd[@]}" logs --no-color --tail=100 ugoite 2>/dev/null || true
  exit 1
fi
if [ "$distinct_secret_count" -ne 1 ]; then
  echo "✗ ERROR: setup secret is ambiguous in the container startup log (occurrences=${secret_count}, distinct=${distinct_secret_count}); refusing to guess"
  echo "  container log tail (ugoite, last 100 lines):"
  "${compose_cmd[@]}" logs --no-color --tail=100 ugoite 2>/dev/null || true
  exit 1
fi
E2E_SETUP_SECRET="$(printf '%s' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | tail -n 1)"
if [ -z "$E2E_SETUP_SECRET" ]; then
  echo "✗ ERROR: setup secret was not present in the container startup log"
  exit 1
fi
export E2E_SETUP_SECRET
echo "  ✓ setup secret extracted exactly once (distinct=1)"

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
  mobile-ui)
    run_e2e_task mobile-ui "$base_report_file"
    ;;
  qry02)
    run_e2e_task qry02 "$base_report_file"
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
    echo "Usage: ./run-e2e-compose.sh [smoke|asset-owned|smoke-and-asset-owned|owner-recovery|mobile-ui|qry02|entries|screenshot|full]"
    exit 1
    ;;
esac

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
