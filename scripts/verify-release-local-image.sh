#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORK_ROOT="${UGOITE_RELEASE_LOCAL_WORKDIR:-$(mktemp -d)}"
KEEP_WORK_ROOT="${UGOITE_RELEASE_LOCAL_KEEP_WORKDIR:-0}"
PROJECT="ugoite-release-local-$$"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/rust}"
case "$CARGO_TARGET_DIR" in
  /*) CLI_BINARY="${CARGO_TARGET_DIR}/release/ugoite" ;;
  *) CLI_BINARY="${REPO_ROOT}/${CARGO_TARGET_DIR}/release/ugoite" ;;
esac

log() {
  printf '%s\n' "$*" >&2
}

fail() {
  log "$*"
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  (
    cd "$WORK_ROOT" &&
      docker compose -p "$PROJECT" -f "$REPO_ROOT/docker-compose.yaml" down --remove-orphans -v
  ) >/dev/null 2>&1 || true
  if [ "$KEEP_WORK_ROOT" != "1" ] && [ -z "${UGOITE_RELEASE_LOCAL_WORKDIR:-}" ]; then
    rm -rf "$WORK_ROOT"
  else
    log "Retained release local workdir: $WORK_ROOT"
  fi
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

require_command cargo
require_command curl
require_command deno
require_command docker

mkdir -p "$WORK_ROOT/spaces" "$WORK_ROOT/node" "$WORK_ROOT/cli-home"
chmod 0777 "$WORK_ROOT/spaces" "$WORK_ROOT/node"

log "Building release CLI binary"
(
  cd "$REPO_ROOT" &&
    cargo build -p ugoite-cli --release --locked
)

log "Building and starting release-like single-image container"
(
  cd "$WORK_ROOT" &&
    docker compose -p "$PROJECT" -f "$REPO_ROOT/docker-compose.yaml" up -d --build
)

HOST_PORT="$(
  cd "$WORK_ROOT" &&
    docker compose -p "$PROJECT" -f "$REPO_ROOT/docker-compose.yaml" port ugoite 8000 |
      awk -F: '{print $NF}'
)"
if [ -z "$HOST_PORT" ]; then
  fail "Could not discover release-like container host port"
fi

APP_URL="http://127.0.0.1:${HOST_PORT}"
API_URL="${APP_URL}/api"

bash "$SCRIPT_DIR/wait-for-http.sh" "${APP_URL}/health" 180
bash "$SCRIPT_DIR/wait-for-http.sh" "${APP_URL}/login" 180

HOME="$WORK_ROOT/cli-home" "$CLI_BINARY" config set --mode api --api-url "$API_URL" >/dev/null
curl -fsS "${API_URL}/auth/config" | grep -Fq '"status":"uninitialized"' ||
  fail "release-like node did not start uninitialized"
curl -fsS "${APP_URL}/.well-known/oauth-protected-resource" >/dev/null
status="$(curl -sS -o /dev/null -w '%{http_code}' "${API_URL}/spaces")"
[ "$status" = "401" ] || fail "unauthenticated Space listing must return 401 (got ${status})"

curl -fsS "${APP_URL}/" | grep -Fqi "<html" || fail "release-like root did not serve frontend HTML"
log "Release-like local image verification passed at ${APP_URL}"
