#!/bin/bash
# E2E test runner script
# Usage: ./scripts/run-e2e.sh [test-type]
#   test-type: "smoke", "notes", or "full" (runs all tests)
#
# Environment variables:
#   E2E_TEST_TIMEOUT_MS: per-test timeout passed to `playwright test --timeout`
#   E2E_FRONTEND_MODE: "dev" (default) or "prod" to use build+start for SSR speed

set -e

# Unset VIRTUAL_ENV to ensure we're using the environment managed by mise/uv 
# and not inheriting an active virtualenv from the current shell session.
unset VIRTUAL_ENV
export BASELINE_BROWSER_MAPPING_IGNORE_OLD_DATA=true
export BROWSERSLIST_IGNORE_OLD_DATA=true

TEST_TYPE="${1:-full}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Kill any existing processes on required ports
echo "Checking for existing processes on ports 8000 and 3000..."
fuser -k 8000/tcp 2>/dev/null || true
fuser -k 3000/tcp 2>/dev/null || true
sleep 1

# Create default workspace for tests
echo "Creating default workspace..."
cd "$ROOT_DIR/backend"
E2E_STORAGE_ROOT="${E2E_STORAGE_ROOT:-}"
if [ -z "$E2E_STORAGE_ROOT" ]; then
    E2E_STORAGE_ROOT="/tmp/ieapp-e2e"
    CLEANUP_E2E_STORAGE=true
else
    CLEANUP_E2E_STORAGE=false
fi

mkdir -p "$E2E_STORAGE_ROOT"

IEAPP_ROOT="$E2E_STORAGE_ROOT" uv run python - <<'PY'
import asyncio

import ieapp_core

from app.core.config import get_root_path
from app.core.storage import storage_config_from_root


async def main() -> None:
    config = storage_config_from_root(get_root_path())
    try:
        await ieapp_core.create_workspace(config, "default")
    except RuntimeError as exc:
        if "already exists" not in str(exc).lower():
            raise


asyncio.run(main())
PY

# Start backend in background
echo "Starting backend server..."
cd "$ROOT_DIR/backend"
IEAPP_ROOT="$E2E_STORAGE_ROOT" IEAPP_ALLOW_REMOTE=true uv run uvicorn src.app.main:app --host 0.0.0.0 --port 8000 &
BACKEND_PID=$!

# Start frontend in background
echo "Starting frontend server..."
cd "$ROOT_DIR/frontend"
FRONTEND_MODE="${E2E_FRONTEND_MODE:-dev}"
if [ "$FRONTEND_MODE" = "prod" ]; then
    echo "Building frontend for production..."
    BACKEND_URL=http://localhost:8000 bun run build
    echo "Starting production frontend server..."
    BACKEND_URL=http://localhost:8000 NODE_ENV=production bun run start &
else
    BACKEND_URL=http://localhost:8000 bun run dev &
fi
FRONTEND_PID=$!

# Cleanup function
cleanup() {
    echo ""
    echo "Stopping servers..."
    kill $BACKEND_PID $FRONTEND_PID 2>/dev/null || true
    wait $BACKEND_PID $FRONTEND_PID 2>/dev/null || true
    echo "Servers stopped."
    if [ "$CLEANUP_E2E_STORAGE" = true ]; then
        rm -rf "$E2E_STORAGE_ROOT"
    fi
}
trap cleanup EXIT INT TERM

# Wait for backend to be ready
echo "Waiting for backend (port 8000)..."
for i in {1..30}; do
    if curl -s http://localhost:8000/workspaces > /dev/null 2>&1; then
        echo "✓ Backend is ready!"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "✗ ERROR: Backend failed to start within 30 seconds"
        exit 1
    fi
    sleep 1
done

# Wait for frontend to be ready
echo "Waiting for frontend (port 3000)..."
for i in {1..60}; do
    if curl -s http://localhost:3000 > /dev/null 2>&1; then
        echo "✓ Frontend is ready!"
        break
    fi
    if [ $i -eq 60 ]; then
        echo "✗ ERROR: Frontend failed to start within 60 seconds"
        exit 1
    fi
    sleep 1
done

# Run tests using Playwright
echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"

TEST_TIMEOUT_ARGS=()
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
    TEST_TIMEOUT_ARGS=(--timeout "${E2E_TEST_TIMEOUT_MS}")
fi

case "$TEST_TYPE" in
    smoke)
            npm run test:smoke -- "${TEST_TIMEOUT_ARGS[@]}"
        ;;
    notes)
            npm run test:notes -- "${TEST_TIMEOUT_ARGS[@]}"
        ;;
    full)
            npm run test -- "${TEST_TIMEOUT_ARGS[@]}"
        ;;
    *)
        echo "Unknown test type: $TEST_TYPE"
        echo "Usage: ./scripts/run-e2e.sh [smoke|notes|full]"
        exit 1
        ;;
esac

echo ""
echo "=========================================="
echo "E2E tests completed!"
echo "=========================================="
