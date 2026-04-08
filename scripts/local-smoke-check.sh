#!/usr/bin/env bash
set -euo pipefail

backend_url="${BACKEND_URL:-http://localhost:8000}"
frontend_url="${FRONTEND_URL:-http://localhost:3000}"

echo "Checking backend health: ${backend_url}/health"
curl -fsS "${backend_url}/health" >/dev/null

if [ -n "${UGOITE_AUTH_BEARER_TOKEN:-}" ]; then
  echo "Checking backend spaces endpoint: ${backend_url}/spaces"
  curl -fsS \
    -H "Authorization: Bearer ${UGOITE_AUTH_BEARER_TOKEN}" \
    "${backend_url}/spaces" >/dev/null
else
  echo "Checking backend auth config: ${backend_url}/auth/config"
  curl -fsS "${backend_url}/auth/config" >/dev/null
fi

echo "Checking frontend root: ${frontend_url}"
curl -fsS "${frontend_url}" >/dev/null

echo "Local smoke check passed"
