#!/bin/bash
# E2E test runner script
# Usage: ./scripts/run-e2e.sh [test-type]
#   test-type: "smoke", "notes", or "full" (runs all tests)
#
# Environment variables:

set -e

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
uv run python -c "from ieapp.workspace import create_workspace; create_workspace('.', 'default')"

# Start backend in background
echo "Starting backend server..."
cd "$ROOT_DIR/backend"
IEAPP_ALLOW_REMOTE=true uv run uvicorn src.app.main:app --host 0.0.0.0 --port 8000 &
BACKEND_PID=$!

# Start frontend in background
echo "Starting frontend server..."
cd "$ROOT_DIR/frontend"
BACKEND_URL=http://localhost:8000 bun run dev &
FRONTEND_PID=$!

# Cleanup function
cleanup() {
    echo ""
    echo "Stopping servers..."
    kill $BACKEND_PID $FRONTEND_PID 2>/dev/null || true
    wait $BACKEND_PID $FRONTEND_PID 2>/dev/null || true
    echo "Servers stopped."
}
trap cleanup EXIT INT TERM

# Wait for backend to be ready
echo "Waiting for backend (port 8000)..."
for i in {1..30}; do
    if curl -s http://localhost:8000/health > /dev/null 2>&1; then
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

# Run tests using Bun's native test runner
echo ""
echo "=========================================="
echo "Running E2E tests (type: $TEST_TYPE)..."
echo "=========================================="

cd "$ROOT_DIR/e2e"

case "$TEST_TYPE" in
    smoke)
        bun test smoke.test.ts
        ;;
    notes)
        bun test notes.test.ts
        ;;
    full)
        bun test
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
