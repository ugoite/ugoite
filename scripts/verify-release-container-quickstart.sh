#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION_INPUT="${UGOITE_VERSION:-}"
WORK_ROOT_INPUT="${UGOITE_QUICKSTART_WORKDIR:-}"
KEEP_WORK_ROOT="${UGOITE_QUICKSTART_KEEP_WORKDIR:-0}"
ASSET_BASE_URL_INPUT="${UGOITE_RELEASE_ASSET_BASE_URL:-}"
CLI_INSTALL_DIR_INPUT="${UGOITE_INSTALL_DIR:-}"
STACK_TIMEOUT_SECONDS="${UGOITE_STACK_START_TIMEOUT_SECONDS:-120}"

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

deno_json_query() {
  local expression="$1"
  local json_input="$2"
  JSON_INPUT="$json_input" deno eval "
const value = JSON.parse(Deno.env.get('JSON_INPUT') ?? '');
const result = (${expression});
if (typeof result === 'string') {
  console.log(result);
} else {
  console.log(JSON.stringify(result));
}
"
}

if [ -z "$VERSION_INPUT" ]; then
  fail "UGOITE_VERSION must be set to the exact release version to verify"
fi

require_command bash
require_command curl
require_command deno
require_command docker

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
CLI_INSTALL_DIR="${CLI_INSTALL_DIR_INPUT:-$CLI_HOME/.local/bin}"
CLI_BINARY="$CLI_INSTALL_DIR/ugoite"
ASSET_BASE_URL="${ASSET_BASE_URL_INPUT:-https://github.com/ugoite/ugoite/releases/download/v${VERSION_INPUT}}"
COMPOSE_PROJECT="ugoite-release-quickstart-${VERSION_INPUT//[^A-Za-z0-9]/-}-$$"
compose_cmd=(docker compose -p "$COMPOSE_PROJECT" -f docker-compose.release.yaml)

mkdir -p "$STACK_DIR/spaces" "$DOWNLOAD_DIR" "$CLI_INSTALL_DIR"
chmod 0777 "$STACK_DIR/spaces"

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
    rm -rf "$WORK_ROOT"
  else
    log "Retained quick-start workdir: $WORK_ROOT"
  fi

  exit "$status"
}
trap cleanup EXIT HUP INT TERM

log "Downloading released container quick-start assets for ${VERSION_INPUT}"
download_asset "docker-compose.release.yaml" "$STACK_DIR/docker-compose.release.yaml"
download_asset "ugoite-v${VERSION_INPUT}.docker.tar.gz" "$DOWNLOAD_DIR/ugoite-image.tar.gz"

log "Loading released Docker image"
gzip -dc "$DOWNLOAD_DIR/ugoite-image.tar.gz" | docker load

cat >"$STACK_DIR/.env" <<EOF
UGOITE_VERSION=${VERSION_INPUT}
UGOITE_SPACES_DIR=./spaces
UGOITE_PORT=8000
UGOITE_DEV_AUTH_MODE=mock-oauth
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_BOOTSTRAP_TOKEN=dev-token
EOF

log "Starting released compose stack"
(
  cd "$STACK_DIR" &&
    "${compose_cmd[@]}" up -d
)

log "Waiting for Ugoite"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:8000/health" \
  "$STACK_TIMEOUT_SECONDS"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:8000/login" \
  "$STACK_TIMEOUT_SECONDS"

login_payload="$(curl -fsS -X POST http://127.0.0.1:8000/api/auth/mock-oauth)"
E2E_AUTH_BEARER_TOKEN="$(deno_json_query "value.bearer_token" "$login_payload")"
if [ -z "$E2E_AUTH_BEARER_TOKEN" ] || [ "$E2E_AUTH_BEARER_TOKEN" = "null" ]; then
  fail "release quick-start mock OAuth did not return a bearer token"
fi
export E2E_AUTH_BEARER_TOKEN

log "Running release browser quick-start stories"
(
  cd "$REPO_ROOT/e2e"
  FRONTEND_URL="http://127.0.0.1:8000" \
    BACKEND_URL="http://127.0.0.1:8000/api" \
    E2E_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
    deno task smoke
)

log "Installing released CLI for remote backend verification"
HOME="$CLI_HOME" \
  PATH="$CLI_INSTALL_DIR:$PATH" \
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

HOME="$CLI_HOME" PATH="$CLI_INSTALL_DIR:$PATH" "$CLI_BINARY" \
  config set --mode api --api-url http://127.0.0.1:8000/api >/dev/null

profile_output="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_INSTALL_DIR:$PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" auth profile
)"
active_token="$(deno_json_query "value.UGOITE_AUTH_BEARER_TOKEN ?? ''" "$profile_output")"
if [ -z "$active_token" ]; then
  fail "missing active CLI bearer token profile"
fi
log "Verified: CLI auth profile exposes a bearer token"

space_list_before="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_INSTALL_DIR:$PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" space list
)"
has_default="$(deno_json_query "value.some((item) => item?.name === 'default')" "$space_list_before")"
if [ "$has_default" != "true" ]; then
  fail "default space missing from remote CLI space list"
fi
log "Verified: remote CLI can list release backend spaces"

remote_space="release-quickstart-remote-$(date +%s)"
HOME="$CLI_HOME" \
  PATH="$CLI_INSTALL_DIR:$PATH" \
  UGOITE_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
  "$CLI_BINARY" space create "$remote_space" >/dev/null
space_list_after="$(
  HOME="$CLI_HOME" \
    PATH="$CLI_INSTALL_DIR:$PATH" \
    UGOITE_AUTH_BEARER_TOKEN="$E2E_AUTH_BEARER_TOKEN" \
    "$CLI_BINARY" space list
)"
has_remote="$(REMOTE_SPACE="$remote_space" deno_json_query "value.some((item) => item?.name === Deno.env.get('REMOTE_SPACE'))" "$space_list_after")"
if [ "$has_remote" != "true" ]; then
  fail "${remote_space} missing from remote CLI space list"
fi
log "Verified: remote CLI can create and observe release backend spaces"

log "Release container quick-start verification passed for ${VERSION_INPUT}"
