#!/usr/bin/env bash
set -euo pipefail

backend_port="${E2E_BACKEND_PORT:-8000}"
frontend_port="${E2E_FRONTEND_PORT:-3000}"

describe_port() {
  local port="$1"
  echo "Port ${port}:"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"${port}" -sTCP:LISTEN || true
  else
    ss -ltnp "( sport = :${port} )" || true
  fi
}

kill_port() {
  local port="$1"
  fuser -k "${port}/tcp" 2>/dev/null || true
}

describe_port "$backend_port"
describe_port "$frontend_port"

kill_port "$backend_port"
kill_port "$frontend_port"

echo "Cleaned stale servers on ports ${backend_port} and ${frontend_port}"
