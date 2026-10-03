#!/bin/bash
# E2E test runner using Docker Compose with locally built or pre-built images.
# Used by local `mise run e2e` and by GitHub Actions e2e-ci.yml.
#
# Usage: ./e2e/scripts/run-e2e-compose.sh [test-type] [--fixture-root PATH]
#   test-type: "smoke", "composition-golden", "asset-owned", "smoke-and-asset-owned",
#     "owner-recovery", "portable-space", "mobile-ui", "query-measurement",
#     "entries", "screenshot", or "full"
#   --fixture-root: caller-owned storage root used by query-measurement
#
# Environment variables:
#   E2E_BUILD_IMAGES: "true" (default) to build local images before startup;
#     "false" to reuse pre-built images (used in CI)
#   E2E_BACKEND_START_TIMEOUT_SECONDS:
#     optional startup wait budget for the composed service (default: 120 seconds)
#   E2E_TEST_TIMEOUT_MS: optional per-test timeout passed to Playwright

set -e

TEST_TYPE="full"
FIXTURE_ROOT=""
if [ "$#" -gt 0 ] && [[ "$1" != --* ]]; then
  TEST_TYPE="$1"
  shift
fi
while [ "$#" -gt 0 ]; do
  case "$1" in
    --fixture-root)
      FIXTURE_ROOT="${2:?missing value for --fixture-root}"
      shift 2
      ;;
    -h|--help)
      sed -n '1,14p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker-compose.e2e.yml"
source "$SCRIPT_DIR/run-e2e-task.sh"

if [ -n "$FIXTURE_ROOT" ] && [ "$TEST_TYPE" != "query-measurement" ]; then
  echo "--fixture-root is only valid for query-measurement" >&2
  exit 1
fi
if [ "$TEST_TYPE" = "query-measurement" ] && [ -z "$FIXTURE_ROOT" ]; then
  echo "query-measurement requires --fixture-root with a preseeded storage root" >&2
  exit 1
fi
if [ -n "$FIXTURE_ROOT" ]; then
  if [ ! -d "$FIXTURE_ROOT" ] || [ -L "$FIXTURE_ROOT" ]; then
    echo "Fixture root must be an existing directory and not a symlink: $FIXTURE_ROOT" >&2
    exit 1
  fi
  FIXTURE_ROOT="$(cd "$FIXTURE_ROOT" && pwd -P)"
  if [ "$FIXTURE_ROOT" = "/" ] || [ "$FIXTURE_ROOT" = "$ROOT_DIR" ] || [ "$FIXTURE_ROOT" = "${HOME:-}" ]; then
    echo "Refusing an unsafe fixture root: $FIXTURE_ROOT" >&2
    exit 1
  fi
  if [ ! -d "$FIXTURE_ROOT/spaces" ] || [ -L "$FIXTURE_ROOT/spaces" ] \
    || [ -z "$(find "$FIXTURE_ROOT/spaces" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    echo "Fixture root must contain a non-empty spaces directory: $FIXTURE_ROOT" >&2
    exit 1
  fi
  if find "$FIXTURE_ROOT" -mindepth 1 -type l -print -quit | grep -q .; then
    echo "Fixture root must not contain symbolic links: $FIXTURE_ROOT" >&2
    exit 1
  fi
fi
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

STORAGE_ROOT_OWNED=true
if [ -n "$FIXTURE_ROOT" ]; then
  E2E_COMPOSE_STORAGE_ROOT="$FIXTURE_ROOT"
  STORAGE_ROOT_OWNED=false
else
  E2E_COMPOSE_STORAGE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-compose-e2e.XXXXXX")"
fi
STORAGE_ROOT_OWNERSHIP_ATTEMPTED=false
PORTABLE_CLI_CONFIG=""
PORTABLE_PROOF_HOST_FILE=""
export E2E_COMPOSE_STORAGE_ROOT

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
  local exit_status=$?
  local cleanup_status=0
  if [ "$exit_status" -ne 0 ]; then
    echo ""
    echo "Service diagnostics (setup secrets redacted):"
    "${compose_cmd[@]}" logs --no-color ugoite 2>/dev/null \
      | sed -E 's/(#secret=)[^[:space:]]+/\1[redacted]/g' \
      | tail -n 120 || true
  fi
  echo ""
  echo "Stopping services..."
  "${compose_cmd[@]}" down -v 2>/dev/null || true
  if [ "$STORAGE_ROOT_OWNERSHIP_ATTEMPTED" = true ] && [ -d "$E2E_COMPOSE_STORAGE_ROOT" ]; then
    # Restore the caller's ownership without changing private file modes. A
    # supplied fixture root belongs to its caller and is never removed here.
    docker run --rm \
      --user 0:0 \
      --volume "$E2E_COMPOSE_STORAGE_ROOT:/data" \
      --entrypoint /bin/sh \
      "${UGOITE_IMAGE_TAG:-ugoite:e2e}" \
      -c "chown -R $(id -u):$(id -g) /data" || cleanup_status=1
  fi
  if [ "$STORAGE_ROOT_OWNED" = true ] && [ -d "$E2E_COMPOSE_STORAGE_ROOT" ]; then
    rm -rf "$E2E_COMPOSE_STORAGE_ROOT" || cleanup_status=1
  fi
  if [ -n "$PORTABLE_CLI_CONFIG" ]; then
    rm -f "$PORTABLE_CLI_CONFIG" || cleanup_status=1
  fi
  if [ -n "$PORTABLE_PROOF_HOST_FILE" ]; then
    rm -f "$PORTABLE_PROOF_HOST_FILE" || cleanup_status=1
  fi
  if [ -n "${DEV_BUILD_INFO_BACKUP:-}" ] && [ -f "$DEV_BUILD_INFO_BACKUP" ]; then
    mv "$DEV_BUILD_INFO_BACKUP" "$DEV_BUILD_INFO_PATH" || cleanup_status=1
  fi
  echo "Services stopped."
  if [ "$exit_status" -eq 0 ] && [ "$cleanup_status" -ne 0 ]; then
    exit_status=$cleanup_status
  fi
  exit "$exit_status"
}
trap cleanup EXIT INT TERM

if [ "$TEST_TYPE" = "portable-space" ]; then
  echo "Seeding a CLI-core Space before Node startup..."
  PORTABLE_CLI_CONFIG="${E2E_COMPOSE_STORAGE_ROOT}.cli-config.toml"
  PORTABLE_PROOF_HOST_FILE="$(mktemp "${TMPDIR:-/tmp}/ugoite-portable-proof.XXXXXX")"
  export E2E_PORTABLE_RUNNER_COMMAND="bash e2e/scripts/run-e2e-compose.sh portable-space"
  bash "$SCRIPT_DIR/seed-portable-space.sh" "$E2E_COMPOSE_STORAGE_ROOT" >/dev/null
  # The proof is read by the host Playwright process. Keep it outside the
  # private Space tree so runtime ownership can stay restricted to ugoite.
  cp "$E2E_COMPOSE_STORAGE_ROOT/portable-space-proof.json" "$PORTABLE_PROOF_HOST_FILE"
  chmod 600 "$PORTABLE_PROOF_HOST_FILE"
  export E2E_PORTABLE_PROOF_FILE="$PORTABLE_PROOF_HOST_FILE"
fi

if [ "$BUILD_IMAGES" = "true" ]; then
  echo "Building services via docker-compose.e2e.yml..."
  "${compose_cmd[@]}" build
fi

# Preserve owner-only modes while granting the image's existing non-root user
# access to the bind mount. This changes ownership only; it never chmods Space data.
STORAGE_ROOT_OWNERSHIP_ATTEMPTED=true
docker run --rm \
  --user 0:0 \
  --volume "$E2E_COMPOSE_STORAGE_ROOT:/data" \
  --entrypoint /bin/sh \
  "${UGOITE_IMAGE_TAG:-ugoite:e2e}" \
  -c 'chown -R ugoite:ugoite /data'

echo "Starting services via docker-compose.e2e.yml..."
if [ "$TEST_TYPE" = "portable-space" ]; then
  "${compose_cmd[@]}" up --no-start
  "${compose_cmd[@]}" start
else
  "${compose_cmd[@]}" up -d
fi

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
secret_count="$(printf '%s\n' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | wc -l | tr -d ' ')"
distinct_secret_count="$(printf '%s\n' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | sort -u | wc -l | tr -d ' ')"
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
E2E_SETUP_SECRET="$(printf '%s\n' "$setup_log" | sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | tail -n 1)"
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

case "$TEST_TYPE" in
  smoke)
    run_e2e_task smoke "$base_report_file" true
    ;;
  composition-golden)
    run_e2e_task composition-golden "$base_report_file" true
    ;;
  asset-owned)
    run_e2e_task asset-owned "$base_report_file" true
    ;;
  smoke-and-asset-owned)
    run_e2e_task smoke-and-asset-owned "$base_report_file" true
    ;;
  owner-recovery)
    run_e2e_task owner-recovery "$base_report_file" true
    ;;
  portable-space)
    run_e2e_task portable-space "$base_report_file" true
    # Let the host CLI read the Space, then restore the runtime owner before
    # the composed service is stopped.
    "${compose_cmd[@]}" run --rm --no-deps --user 0:0 --entrypoint /bin/sh ugoite \
      -c "chown -R $(id -u):$(id -g) /data"
    echo "Verifying copied authoritative file hashes..."
    deno run -A "$SCRIPT_DIR/verify-portable-space-hashes.ts" \
      "$E2E_COMPOSE_STORAGE_ROOT" "$E2E_PORTABLE_PROOF_FILE"
    echo "Verifying the claimed copied Space with the local CLI..."
    cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
      -- --config "$PORTABLE_CLI_CONFIG" space verify --deep --format json > "$E2E_COMPOSE_STORAGE_ROOT/verify-after-claim.json"
    deno eval '
      const report = JSON.parse(await Deno.readTextFile(Deno.args[0]));
      if (!["valid", "valid_with_rebuildable_derived_state"].includes(report.status)) throw new Error(`post-claim Space status was ${report.status}`);
      if (report.sections.authorization.status !== "valid") throw new Error(`post-claim authorization status was ${report.sections.authorization.status}`);
    ' "$E2E_COMPOSE_STORAGE_ROOT/verify-after-claim.json"
    "${compose_cmd[@]}" run --rm --no-deps --user 0:0 --entrypoint /bin/sh ugoite \
      -c 'chown -R ugoite:ugoite /data'
    ;;
  mobile-ui)
    run_e2e_task mobile-ui "$base_report_file" true
    ;;
  query-measurement)
    run_e2e_task query-measurement "$base_report_file" true
    ;;
  entries)
    run_e2e_task entries "$base_report_file" true
    ;;
  screenshot)
    run_e2e_task screenshot "$base_report_file" true
    ;;
  full)
    run_e2e_task full "$base_report_file" true
    ;;
  *)
    echo "Unknown test type: $TEST_TYPE"
    echo "Usage: ./run-e2e-compose.sh [smoke|composition-golden|asset-owned|smoke-and-asset-owned|owner-recovery|portable-space|mobile-ui|query-measurement|entries|screenshot|full] [--fixture-root PATH]"
    exit 1
    ;;
esac

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
