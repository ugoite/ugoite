#!/usr/bin/env bash
set -euo pipefail

backend_url="${BACKEND_URL:-http://localhost:8000}"
frontend_url="${FRONTEND_URL:-http://localhost:3000}"

echo "Checking backend health: ${backend_url}/health"
curl -fsS "${backend_url}/health" > /dev/null

echo "Checking backend spaces endpoint: ${backend_url}/spaces"
curl -fsS "${backend_url}/spaces" > /dev/null

echo "Checking frontend root: ${frontend_url}"
curl -fsS "${frontend_url}" > /dev/null

echo "Local smoke check passed"
