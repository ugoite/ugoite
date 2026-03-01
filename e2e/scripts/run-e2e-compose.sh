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

UGOITE_AUTH_BEARER_TOKENS_JSON="{\"$E2E_AUTH_BEARER_TOKEN\":{\"user_id\":\"e2e-user\",\"principal_type\":\"user\"},\"alice-token\":{\"user_id\":\"alice-user\",\"principal_type\":\"user\"},\"bob-token\":{\"user_id\":\"bob-user\",\"principal_type\":\"user\"}}"
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
    if curl -sf "http://localhost:8000/spaces" -H "Authorization: Bearer $E2E_AUTH_BEARER_TOKEN" > /dev/null 2>&1; then
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

# Create default space
echo "Creating default space..."
docker compose -f "$ROOT_DIR/docker-compose.e2e.yml" exec -T backend \
    uv run python - <<'PY'
import asyncio
import os

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

# Wait for frontend to be ready
echo "Waiting for frontend (port 3000)..."
for i in $(seq 1 60); do
    if curl -sf "http://localhost:3000" > /dev/null 2>&1; then
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
npm run test -- ${E2E_TEST_TIMEOUT_MS:+--timeout $E2E_TEST_TIMEOUT_MS}

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
