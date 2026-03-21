#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION_INPUT="${UGOITE_VERSION:-}"
WORK_ROOT_INPUT="${UGOITE_QUICKSTART_WORKDIR:-}"
KEEP_WORK_ROOT="${UGOITE_QUICKSTART_KEEP_WORKDIR:-0}"
ASSET_BASE_URL_INPUT="${UGOITE_RELEASE_ASSET_BASE_URL:-}"
BACKEND_TIMEOUT_SECONDS="${UGOITE_BACKEND_START_TIMEOUT_SECONDS:-120}"
FRONTEND_TIMEOUT_SECONDS="${UGOITE_FRONTEND_START_TIMEOUT_SECONDS:-120}"
CLI_INSTALL_DIR_INPUT="${UGOITE_INSTALL_DIR:-}"

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

download_asset() {
  local asset_name="$1"
  local output_path="$2"
  local url="${ASSET_BASE_URL}/${asset_name}"
  local attempt

  for attempt in $(seq 1 10); do
    if curl -fsSL -o "$output_path" "$url"; then
      return 0
    fi
    if [ "$attempt" -eq 10 ]; then
      fail "Failed to download ${asset_name} from ${url} after ${attempt} attempts"
    fi
    sleep 3
  done
}

if [ -z "$VERSION_INPUT" ]; then
  fail "UGOITE_VERSION must be set to the exact release version to verify"
fi

require_command curl
require_command docker
require_command npm
require_command npx
require_command python3
require_command bash

cleanup_mode="cleanup"
if [ -n "$WORK_ROOT_INPUT" ]; then
  WORK_ROOT="$WORK_ROOT_INPUT"
  mkdir -p "$WORK_ROOT"
  cleanup_mode="keep"
else
  WORK_ROOT="$(mktemp -d)"
fi

if [ "$KEEP_WORK_ROOT" = "1" ]; then
  cleanup_mode="keep"
fi

STACK_DIR="$WORK_ROOT/release-stack"
DOWNLOAD_DIR="$WORK_ROOT/release-assets"
CLI_HOME="$WORK_ROOT/cli-home"
if [ -n "$CLI_INSTALL_DIR_INPUT" ]; then
  CLI_INSTALL_DIR="$CLI_INSTALL_DIR_INPUT"
else
  CLI_INSTALL_DIR="$CLI_HOME/.local/bin"
fi
CLI_BINARY="$CLI_INSTALL_DIR/ugoite"
ASSET_BASE_URL="${ASSET_BASE_URL_INPUT:-https://github.com/ugoite/ugoite/releases/download/v${VERSION_INPUT}}"
COMPOSE_PROJECT="ugoite-release-quickstart-${VERSION_INPUT//[^A-Za-z0-9]/-}-$$"
compose_cmd=(docker compose -p "$COMPOSE_PROJECT" -f docker-compose.release.yaml)
PLAYWRIGHT_TESTS=(
  smoke.test.ts
  search-ui.test.ts
)

mkdir -p "$STACK_DIR/spaces" "$DOWNLOAD_DIR" "$CLI_INSTALL_DIR"

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM

  if [ -f "$STACK_DIR/docker-compose.release.yaml" ]; then
    if [ "$status" -ne 0 ]; then
      log "Release quick-start verification failed; compose logs follow."
      (
        cd "$STACK_DIR" &&
          "${compose_cmd[@]}" logs --no-color
      ) || true
    fi

    (
      cd "$STACK_DIR" &&
        "${compose_cmd[@]}" down --remove-orphans -v
    ) || true
  fi

  if [ "$cleanup_mode" = "cleanup" ]; then
    if ! rm -rf "$WORK_ROOT" 2>/dev/null; then
      log "Host cleanup hit permission issues; retrying inside a container."
      docker run --rm \
        -v "$WORK_ROOT:/work" \
        --entrypoint /bin/sh \
        "ghcr.io/ugoite/ugoite/backend:${VERSION_INPUT}" \
        -c 'rm -rf /work/* /work/.[!.]* /work/..?*'
      rm -rf "$WORK_ROOT"
    fi
  else
    log "Retained quick-start workdir: $WORK_ROOT"
  fi

  exit "$status"
}
trap cleanup EXIT HUP INT TERM

log "Downloading released container quick-start assets for ${VERSION_INPUT}"
download_asset "docker-compose.release.yaml" "$STACK_DIR/docker-compose.release.yaml"
download_asset \
  "ugoite-backend-v${VERSION_INPUT}.docker.tar.gz" \
  "$DOWNLOAD_DIR/backend-image.tar.gz"
download_asset \
  "ugoite-frontend-v${VERSION_INPUT}.docker.tar.gz" \
  "$DOWNLOAD_DIR/frontend-image.tar.gz"

log "Loading released Docker images"
gzip -dc "$DOWNLOAD_DIR/backend-image.tar.gz" | docker load
gzip -dc "$DOWNLOAD_DIR/frontend-image.tar.gz" | docker load

log "Starting released compose stack"
(
  cd "$STACK_DIR" &&
    UGOITE_VERSION="$VERSION_INPUT" "${compose_cmd[@]}" up -d
)

log "Waiting for backend"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:8000/health" \
  "$BACKEND_TIMEOUT_SECONDS"
log "Waiting for frontend"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:3000/login" \
  "$FRONTEND_TIMEOUT_SECONDS"

E2E_AUTH_BEARER_TOKEN="$(
  python3 - <<'PY'
import json
from urllib.request import Request, urlopen

request = Request("http://127.0.0.1:3000/api/auth/dev/mock-oauth", method="POST")
with urlopen(request) as response:
    print(json.load(response)["bearer_token"])
PY
)"
export E2E_AUTH_BEARER_TOKEN

log "Installing Playwright dependencies"
(
  cd "$REPO_ROOT/e2e"
  npm ci
  npx playwright install --with-deps chromium
)

log "Running release browser quick-start stories"
(
  cd "$REPO_ROOT/e2e"
  playwright_junit_output_file="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/release-quickstart-junit.xml}"
  mkdir -p "$(dirname "$playwright_junit_output_file")"
  FRONTEND_URL="http://127.0.0.1:3000" \
    BACKEND_URL="http://127.0.0.1:8000" \
    E2E_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
    PLAYWRIGHT_CI_REPORTER="${PLAYWRIGHT_CI_REPORTER:-junit}" \
    PLAYWRIGHT_JUNIT_OUTPUT_FILE="$playwright_junit_output_file" \
    npx playwright test "${PLAYWRIGHT_TESTS[@]}"
)

log "Installing released CLI for remote backend verification"
CLI_PATH="$CLI_INSTALL_DIR:$PATH"
HOME="$CLI_HOME" \
  PATH="$CLI_PATH" \
  UGOITE_VERSION="$VERSION_INPUT" \
  UGOITE_INSTALL_DIR="$CLI_INSTALL_DIR" \
  /bin/bash "$SCRIPT_DIR/install-ugoite-cli.sh"

if [ ! -x "$CLI_BINARY" ]; then
  fail "Expected installed CLI binary at ${CLI_BINARY}"
fi

help_output="$("$CLI_BINARY" --help 2>&1)"
printf '%s' "$help_output" | grep -Fq "Ugoite CLI - Knowledge base management" || (
  fail "installed CLI did not return the expected --help output"
)
log "Verified: installed CLI answers --help"

HOME="$CLI_HOME" PATH="$CLI_PATH" "$CLI_BINARY" \
  config set --mode backend --backend-url http://127.0.0.1:8000 >/dev/null

login_output="$(HOME="$CLI_HOME" PATH="$CLI_PATH" "$CLI_BINARY" auth login --mock-oauth)"
case "$login_output" in
  export\ UGOITE_AUTH_BEARER_TOKEN=*)
    export UGOITE_AUTH_BEARER_TOKEN="${login_output#export UGOITE_AUTH_BEARER_TOKEN=}"
    ;;
  *)
    fail "auth login --mock-oauth did not print a bearer token export"
    ;;
esac

profile_output="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$UGOITE_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" auth profile
)"
python3 - "$profile_output" <<'PY'
import json
import sys

profile = json.loads(sys.argv[1])
value = profile.get("UGOITE_AUTH_BEARER_TOKEN")
if not isinstance(value, str) or not value:
    raise SystemExit("missing active CLI bearer token profile")
PY
log "Verified: CLI auth profile exposes a bearer token"

space_list_before="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$UGOITE_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" space list
)"
python3 - "$space_list_before" <<'PY'
import json
import sys

spaces = json.loads(sys.argv[1])
if not any(isinstance(item, dict) and item.get("name") == "default" for item in spaces):
    raise SystemExit("default space missing from remote CLI space list")
PY
log "Verified: remote CLI can list release backend spaces"

remote_space="release-quickstart-remote-$(date +%s)"
HOME="$CLI_HOME" \
  PATH="$CLI_PATH" \
  UGOITE_AUTH_BEARER_TOKEN="$UGOITE_AUTH_BEARER_TOKEN" \
  "$CLI_BINARY" create-space "$remote_space" >/dev/null
space_list_after="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$UGOITE_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" space list
)"
python3 - "$remote_space" "$space_list_after" <<'PY'
import json
import sys

space_name = sys.argv[1]
spaces = json.loads(sys.argv[2])
if not any(isinstance(item, dict) and item.get("name") == space_name for item in spaces):
    raise SystemExit(f"{space_name} missing from remote CLI space list")
PY
log "Verified: remote CLI can create and observe a release backend space"

log "Release container quick-start verification passed for ${VERSION_INPUT}"
