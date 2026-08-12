#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION_INPUT="${UGOITE_VERSION:-}"
RELEASE_TAG_INPUT="${UGOITE_RELEASE_TAG:-v${VERSION_INPUT}}"
RELEASE_SHA_INPUT="${UGOITE_RELEASE_SHA:-}"
IMAGE_REPOSITORY="${UGOITE_IMAGE_REPOSITORY:-ghcr.io/ugoite/ugoite}"
RELEASE_TOKEN_INPUT="${UGOITE_RELEASE_TOKEN:-}"
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
  local -a curl_args=(-fsSL)

  if [ -n "$RELEASE_TOKEN_INPUT" ]; then
    curl_args+=(
      -H "Authorization: Bearer ${RELEASE_TOKEN_INPUT}"
      -H "Accept: application/octet-stream"
    )
  fi

  for attempt in $(seq 1 10); do
    if curl "${curl_args[@]}" -o "$output_path" "$url"; then
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

validate_release_manifest() {
  local manifest_path="$1"
  local compose_path="$2"
  local compose_checksum_path="$3"

  EXPECTED_RELEASE_TAG="$RELEASE_TAG_INPUT" \
    EXPECTED_VERSION="$VERSION_INPUT" \
    EXPECTED_SOURCE_SHA="$RELEASE_SHA_INPUT" \
    EXPECTED_IMAGE_REPOSITORY="$IMAGE_REPOSITORY" \
    MANIFEST_PATH="$manifest_path" \
    COMPOSE_PATH="$compose_path" \
    COMPOSE_CHECKSUM_PATH="$compose_checksum_path" \
    deno eval '
const manifest = JSON.parse(await Deno.readTextFile(Deno.env.get("MANIFEST_PATH")!));
const expectedTag = Deno.env.get("EXPECTED_RELEASE_TAG");
const expectedVersion = Deno.env.get("EXPECTED_VERSION");
const expectedSourceSha = Deno.env.get("EXPECTED_SOURCE_SHA");
const expectedImageRepository = Deno.env.get("EXPECTED_IMAGE_REPOSITORY");
const fail = (message: string): never => {
  console.error(`release manifest validation failed: ${message}`);
  Deno.exit(1);
};
if (manifest.release_tag !== expectedTag) fail(`release tag ${manifest.release_tag} != ${expectedTag}`);
if (manifest.version !== expectedVersion) fail(`version ${manifest.version} != ${expectedVersion}`);
if (manifest.source_sha !== expectedSourceSha) fail(`source SHA ${manifest.source_sha} != ${expectedSourceSha}`);
if (manifest.image?.repository !== expectedImageRepository) {
  fail(`image repository ${manifest.image?.repository} != ${expectedImageRepository}`);
}
if (typeof manifest.image?.digest !== "string" || !manifest.image.digest.startsWith("sha256:")) {
  fail("published image digest is missing");
}
const digest = async (path: string): Promise<string> => {
  const bytes = await Deno.readFile(path);
  const hash = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
};
for (const [name, path] of [
  ["docker-compose.release.yaml", Deno.env.get("COMPOSE_PATH")!],
  ["docker-compose.release.yaml.sha256", Deno.env.get("COMPOSE_CHECKSUM_PATH")!],
] as const) {
  const record = manifest.files?.find((file: { name?: string }) => file.name === name);
  if (!record) fail(`${name} is absent from manifest`);
  if (record.sha256 !== await digest(path)) fail(`${name} digest does not match manifest`);
}
console.log(manifest.image.digest);
'
}

if [ -z "$VERSION_INPUT" ]; then
  fail "UGOITE_VERSION must be set to the exact release version to verify"
fi

if [ -z "$RELEASE_SHA_INPUT" ]; then
  fail "UGOITE_RELEASE_SHA must be set to the prepared release commit"
fi

require_command bash
require_command curl
require_command deno
require_command docker
if command -v sha256sum >/dev/null 2>&1; then
  CHECKSUM_COMMAND="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  CHECKSUM_COMMAND="shasum"
else
  fail "Required command not found: sha256sum or shasum"
fi

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
CLI_HOME="$WORK_ROOT/cli-home"
CLI_INSTALL_DIR="${CLI_INSTALL_DIR_INPUT:-$CLI_HOME/.local/bin}"
CLI_BINARY="$CLI_INSTALL_DIR/ugoite"
ASSET_BASE_URL="${ASSET_BASE_URL_INPUT:-https://github.com/ugoite/ugoite/releases/download/v${VERSION_INPUT}}"
COMPOSE_PROJECT="ugoite-release-quickstart-${VERSION_INPUT//[^A-Za-z0-9]/-}-$$"
compose_cmd=(docker compose -p "$COMPOSE_PROJECT" -f docker-compose.release.yaml)
NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64 | tr -d '\n')"

redact_compose_logs() {
  sed -E \
    -e 's|(#secret=)[^[:space:]]+|\1[REDACTED]|g' \
    -e 's|(UGOITE_NODE_SECRET_KEY=)[^[:space:]]+|\1[REDACTED]|g'
}

verify_checksum() {
  local asset_path="$1"
  local checksum_path="$2"

  if [ "$CHECKSUM_COMMAND" = "sha256sum" ]; then
    (cd "$(dirname "$asset_path")" && sha256sum -c "$(basename "$checksum_path")")
    return
  fi

  local expected actual
  expected="$(cut -d ' ' -f 1 <"$checksum_path")"
  actual="$(shasum -a 256 "$asset_path" | awk '{print $1}')"
  [ "$expected" = "$actual" ] || fail "Checksum verification failed for $(basename "$asset_path")"
}

mkdir -p "$STACK_DIR/data" "$CLI_INSTALL_DIR"
chmod 0777 "$STACK_DIR/data"

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM

  if [ -f "$STACK_DIR/docker-compose.release.yaml" ]; then
    if [ "$status" -ne 0 ]; then
      log "Release quick-start verification failed; compose logs follow."
      (
        cd "$STACK_DIR" &&
          UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
            "${compose_cmd[@]}" logs --no-color | redact_compose_logs
      ) || true
    fi
    (
      cd "$STACK_DIR" &&
        UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
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
download_asset "docker-compose.release.yaml.sha256" "$STACK_DIR/docker-compose.release.yaml.sha256"
verify_checksum "$STACK_DIR/docker-compose.release.yaml" "$STACK_DIR/docker-compose.release.yaml.sha256"
download_asset "release-manifest.json" "$STACK_DIR/release-manifest.json"
EXPECTED_IMAGE_DIGEST="$(validate_release_manifest \
  "$STACK_DIR/release-manifest.json" \
  "$STACK_DIR/docker-compose.release.yaml" \
  "$STACK_DIR/docker-compose.release.yaml.sha256")"

cat >"$STACK_DIR/.env" <<EOF
UGOITE_VERSION=${VERSION_INPUT}
UGOITE_DATA_DIR=./data
UGOITE_PORT=8000
UGOITE_PUBLIC_ORIGIN=http://127.0.0.1:8000
UGOITE_API_BASE_URL=http://127.0.0.1:8000/api
UGOITE_WEBAUTHN_RP_ID=127.0.0.1
EOF

log "Starting released compose stack"
compose_images="$(
  cd "$STACK_DIR" &&
    UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
      "${compose_cmd[@]}" config --images
)"
expected_compose_image="${IMAGE_REPOSITORY}:${VERSION_INPUT}"
if [ "$compose_images" != "$expected_compose_image" ]; then
  fail "released Compose references ${compose_images}, expected ${expected_compose_image}"
fi
(
  cd "$STACK_DIR" &&
    UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
      "${compose_cmd[@]}" pull &&
    UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
      "${compose_cmd[@]}" up -d --no-build
)

published_image_digest="$(
  docker buildx imagetools inspect \
    "${IMAGE_REPOSITORY}:${VERSION_INPUT}" \
    --format '{{json .Manifest.Digest}}' | tr -d '"'
)"
if [ "$published_image_digest" != "$EXPECTED_IMAGE_DIGEST" ]; then
  fail "published image digest ${published_image_digest} did not match release manifest ${EXPECTED_IMAGE_DIGEST}"
fi

log "Waiting for Ugoite"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:8000/health" \
  "$STACK_TIMEOUT_SECONDS"
bash "$SCRIPT_DIR/wait-for-http.sh" \
  "http://127.0.0.1:8000/login" \
  "$STACK_TIMEOUT_SECONDS"

E2E_SETUP_SECRET="$(
  cd "$STACK_DIR" &&
    UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
      "${compose_cmd[@]}" logs --no-color ugoite |
      sed -n 's/.*#secret=\([^[:space:]]*\).*/\1/p' | tail -n 1
)"
if [ -z "$E2E_SETUP_SECRET" ]; then
  fail "release quick-start setup secret was not present in startup logs"
fi
export E2E_SETUP_SECRET

log "Running release browser quick-start stories"
(
  cd "$REPO_ROOT/e2e"
  FRONTEND_URL="http://127.0.0.1:8000" \
    BACKEND_URL="http://127.0.0.1:8000" \
    E2E_SETUP_SECRET="$E2E_SETUP_SECRET" \
    deno task smoke
) 2>&1 | redact_compose_logs

unset E2E_SETUP_SECRET
log "Installing released CLI for container quick-start configuration checks"
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

auth_help="$($CLI_BINARY auth --help 2>&1)"
printf '%s' "$auth_help" | grep -Fq "login" || fail "installed CLI does not expose device login"
log "Verified: installed CLI exposes device authorization login"

log "Release container quick-start verification passed for ${VERSION_INPUT}"
