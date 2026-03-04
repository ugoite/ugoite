#!/bin/bash
# E2E test runner using Docker Compose with pre-built images.
# Used by e2e-ci.yml; relies on ugoite-backend:e2e and ugoite-frontend:e2e
# images already loaded into the local Docker daemon.
#
# Environment variables:
#   E2E_AUTH_BEARER_TOKEN: token used by Playwright tests (auto-generated if unset)
#   PLAYWRIGHT_CI_REPORTER / PLAYWRIGHT_JUNIT_OUTPUT_FILE: passed through to tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [ -z "${E2E_AUTH_BEARER_TOKEN:-}" ]; then
  E2E_AUTH_BEARER_TOKEN="$(python3 -c 'import secrets; print(f"e2e-{secrets.token_urlsafe(24)}")')"
fi
export E2E_AUTH_BEARER_TOKEN

UGOITE_AUTH_BEARER_TOKENS_JSON="$(
  python3 - "$E2E_AUTH_BEARER_TOKEN" <<'PY'
import json
import sys

token = sys.argv[1]
print(
    json.dumps(
        {
            token: {"user_id": "e2e-user", "principal_type": "user"},
            "alice-token": {"user_id": "alice-user", "principal_type": "user"},
            "bob-token": {"user_id": "bob-user", "principal_type": "user"},
        }
    )
)
PY
)"
export UGOITE_AUTH_BEARER_TOKENS_JSON
export UGOITE_BOOTSTRAP_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN"

cleanup() {
  echo ""
  echo "Stopping services..."
  docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" down -v 2>/dev/null || true
  echo "Services stopped."
}
trap cleanup EXIT INT TERM

echo "Starting services via docker-compose.e2e.yml..."
docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" up -d

# Wait for backend to be ready
echo "Waiting for backend (port 8000)..."
for i in $(seq 1 60); do
  if curl -sf "http://localhost:8000/spaces" -H "Authorization: Bearer $E2E_AUTH_BEARER_TOKEN" >/dev/null 2>&1; then
    echo "✓ Backend is ready!"
    break
  fi
  if [ "$i" -eq 60 ]; then
    echo "✗ ERROR: Backend failed to start within 60 seconds"
    docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" logs backend
    exit 1
  fi
  sleep 1
done

# Wait for frontend to be ready
echo "Waiting for frontend (port 3000)..."
for i in $(seq 1 60); do
  if curl -sf "http://localhost:3000" >/dev/null 2>&1; then
    echo "✓ Frontend is ready!"
    break
  fi
  if [ "$i" -eq 60 ]; then
    echo "✗ ERROR: Frontend failed to start within 60 seconds"
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
