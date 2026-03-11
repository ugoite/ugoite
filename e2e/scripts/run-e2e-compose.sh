#!/bin/bash
# E2E test runner using Docker Compose with pre-built images.
# Used by e2e-ci.yml; relies on ugoite-backend:e2e and ugoite-frontend:e2e
# images already loaded into the local Docker daemon.
#
# Environment variables:
#   PLAYWRIGHT_CI_REPORTER / PLAYWRIGHT_JUNIT_OUTPUT_FILE: passed through to tests
#   E2E_BACKEND_START_TIMEOUT_SECONDS / E2E_FRONTEND_START_TIMEOUT_SECONDS:
#     optional startup wait budgets for compose services (defaults: 120 seconds)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-e2e-local-signing-secret}"
STATIC_E2E_TOKENS_JSON='{"alice-token":{"user_id":"alice-user","principal_type":"user"},"bob-token":{"user_id":"bob-user","principal_type":"user"}}'
export UGOITE_DEV_AUTH_MODE=mock-oauth
export UGOITE_DEV_USER_ID=e2e-user
export UGOITE_DEV_SIGNING_KID="$DEV_SIGNING_KID"
export UGOITE_DEV_SIGNING_SECRET="$DEV_SIGNING_SECRET"
export UGOITE_AUTH_BEARER_SECRETS="$DEV_SIGNING_KID:$DEV_SIGNING_SECRET"
export UGOITE_AUTH_BEARER_ACTIVE_KIDS="$DEV_SIGNING_KID"
export UGOITE_AUTH_BEARER_TOKENS_JSON="$STATIC_E2E_TOKENS_JSON"

backend_start_timeout="${E2E_BACKEND_START_TIMEOUT_SECONDS:-120}"
frontend_start_timeout="${E2E_FRONTEND_START_TIMEOUT_SECONDS:-120}"

cleanup() {
  echo ""
  echo "Stopping services..."
  docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" down -v 2>/dev/null || true
  echo "Services stopped."
}
trap cleanup EXIT INT TERM

echo "Starting services via docker-compose.e2e.yml..."
docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" up -d

echo "Waiting for backend (port 8000)..."
for i in $(seq 1 "$backend_start_timeout"); do
  if curl -sf "http://localhost:8000/health" >/dev/null 2>&1; then
    echo "✓ Backend is ready!"
    break
  fi
  if [ "$i" -eq "$backend_start_timeout" ]; then
    echo "✗ ERROR: Backend failed to start within ${backend_start_timeout} seconds"
    docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" logs backend
    exit 1
  fi
  sleep 1
done

E2E_AUTH_BEARER_TOKEN="$(
  curl -fsS -X POST http://localhost:8000/auth/dev/mock-oauth | python3 -c 'import json, sys; print(json.load(sys.stdin)["bearer_token"])'
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
    docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" logs frontend
    exit 1
  fi
  sleep 1
done

echo ""
echo "=========================================="
echo "Running E2E tests..."
echo "=========================================="

cd "$ROOT_DIR/e2e"
cmd=(npm run test --)
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
  cmd+=(--timeout "$E2E_TEST_TIMEOUT_MS")
fi
"${cmd[@]}"

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
