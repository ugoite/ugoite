#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_SCRIPT_PATH="${UGOITE_INSTALL_SCRIPT_PATH:-${SCRIPT_DIR}/install-ugoite-cli.sh}"
VERSION_INPUT="${UGOITE_VERSION:-}"
RELEASE_TAG_INPUT="${UGOITE_RELEASE_TAG:-v${VERSION_INPUT}}"
RELEASE_SHA_INPUT="${UGOITE_RELEASE_SHA:-}"
ASSET_BASE_URL_INPUT="${UGOITE_RELEASE_ASSET_BASE_URL:-}"
RELEASE_TOKEN_INPUT="${UGOITE_RELEASE_TOKEN:-}"
CLI_TARGET_INPUT="${UGOITE_CLI_TARGET:-}"
WORK_ROOT_INPUT="${UGOITE_QUICKSTART_WORKDIR:-}"
KEEP_WORK_ROOT="${UGOITE_QUICKSTART_KEEP_WORKDIR:-0}"
QUICKSTART_HOME_INPUT="${UGOITE_QUICKSTART_HOME:-}"
INSTALL_DIR_INPUT="${UGOITE_INSTALL_DIR:-}"
SPACE_ID="${UGOITE_SPACE_ID:-demo}"
SPACE_ROOT="./data/spaces"

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

detect_target() {
  if [ -n "$CLI_TARGET_INPUT" ]; then
    printf '%s' "$CLI_TARGET_INPUT"
    return
  fi

  case "$(uname -s):$(uname -m)" in
    Linux:x86_64) printf '%s' 'x86_64-unknown-linux-gnu' ;;
    Linux:arm64 | Linux:aarch64) printf '%s' 'aarch64-unknown-linux-gnu' ;;
    Darwin:x86_64) printf '%s' 'x86_64-apple-darwin' ;;
    Darwin:arm64 | Darwin:aarch64) printf '%s' 'aarch64-apple-darwin' ;;
    *) fail "Unsupported release CLI target: $(uname -s) $(uname -m)" ;;
  esac
}

verify_checksum() {
  local archive_path="$1"
  local checksum_path="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "$archive_path")" && sha256sum -c "$(basename "$checksum_path")")
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    local expected actual
    expected="$(cut -d ' ' -f 1 <"$checksum_path")"
    actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
    [ "$expected" = "$actual" ] || fail "Checksum verification failed for $(basename "$archive_path")"
    return
  fi

  fail "Need sha256sum or shasum to verify the downloaded archive"
}

validate_release_manifest() {
  local manifest_path="$1"
  local archive_path="$2"
  local checksum_path="$3"
  local archive_name="$4"
  local checksum_name="$5"

  EXPECTED_RELEASE_TAG="$RELEASE_TAG_INPUT" \
    EXPECTED_VERSION="$VERSION_INPUT" \
    EXPECTED_SOURCE_SHA="$RELEASE_SHA_INPUT" \
    MANIFEST_PATH="$manifest_path" \
    ARCHIVE_PATH="$archive_path" \
    CHECKSUM_PATH="$checksum_path" \
    ARCHIVE_NAME="$archive_name" \
    CHECKSUM_NAME="$checksum_name" \
    deno eval '
const manifest = JSON.parse(await Deno.readTextFile(Deno.env.get("MANIFEST_PATH")!));
const expectedTag = Deno.env.get("EXPECTED_RELEASE_TAG");
const expectedVersion = Deno.env.get("EXPECTED_VERSION");
const expectedSourceSha = Deno.env.get("EXPECTED_SOURCE_SHA");
const fail = (message: string): never => {
  console.error(`release manifest validation failed: ${message}`);
  Deno.exit(1);
};
if (manifest.release_tag !== expectedTag) fail(`release tag ${manifest.release_tag} != ${expectedTag}`);
if (manifest.version !== expectedVersion) fail(`version ${manifest.version} != ${expectedVersion}`);
if (expectedSourceSha && manifest.source_sha !== expectedSourceSha) {
  fail(`source SHA ${manifest.source_sha} != ${expectedSourceSha}`);
}
const digest = async (path: string): Promise<string> => {
  const bytes = await Deno.readFile(path);
  const hash = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
};
for (const [name, path] of [
  [Deno.env.get("ARCHIVE_NAME")!, Deno.env.get("ARCHIVE_PATH")!],
  [Deno.env.get("CHECKSUM_NAME")!, Deno.env.get("CHECKSUM_PATH")!],
] as const) {
  const record = manifest.files?.find((file: { name?: string }) => file.name === name);
  if (!record) fail(`${name} is absent from manifest`);
  const bytes = await Deno.readFile(path);
  if (record.size !== bytes.byteLength) fail(`${name} size does not match manifest`);
  if (record.sha256 !== await digest(path)) fail(`${name} digest does not match manifest`);
}
'
}

assert_json_equals() {
  local label="$1"
  local expected_json="$2"
  local actual_json="$3"

  ASSERT_LABEL="$label" \
    EXPECTED_JSON="$expected_json" \
    ACTUAL_JSON="$actual_json" \
    deno eval '
const label = Deno.env.get("ASSERT_LABEL") ?? "JSON assertion";
const expectedRaw = Deno.env.get("EXPECTED_JSON") ?? "";
const actualRaw = Deno.env.get("ACTUAL_JSON") ?? "";

let expected;
let actual;
try {
  expected = JSON.parse(expectedRaw);
} catch (error) {
  console.error(`${label}: expected JSON fixture could not be decoded: ${error.message}: ${expectedRaw}`);
  Deno.exit(1);
}
try {
  actual = JSON.parse(actualRaw);
} catch (error) {
  console.error(`${label}: command output was not valid JSON: ${error.message}: ${actualRaw}`);
  Deno.exit(1);
}
if (JSON.stringify(actual) !== JSON.stringify(expected)) {
  console.error(`${label}: expected ${JSON.stringify(expected)} but got ${JSON.stringify(actual)}`);
  Deno.exit(1);
}
'
}

assert_help_output() {
  local help_output="$1"

  printf '%s' "$help_output" | grep -Fq "Ugoite CLI - Knowledge base management" || (
    fail "installed binary did not return the expected --help output"
  )
}

if [ -z "$VERSION_INPUT" ]; then
  fail "UGOITE_VERSION must be set to the exact release version to verify"
fi

if [ ! -f "$INSTALL_SCRIPT_PATH" ]; then
  fail "Install script not found: $INSTALL_SCRIPT_PATH"
fi

require_command deno
require_command curl

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

cleanup() {
  if [ "$cleanup_mode" = "cleanup" ]; then
    rm -rf "$WORK_ROOT"
    return
  fi

  log "Retained quick-start workdir: $WORK_ROOT"
}
trap cleanup EXIT HUP INT TERM

if [ -n "$QUICKSTART_HOME_INPUT" ]; then
  QUICKSTART_HOME="$QUICKSTART_HOME_INPUT"
else
  QUICKSTART_HOME="$WORK_ROOT/home"
fi
mkdir -p "$QUICKSTART_HOME"

if [ -n "$INSTALL_DIR_INPUT" ]; then
  INSTALL_DIR="$INSTALL_DIR_INPUT"
else
  INSTALL_DIR="$QUICKSTART_HOME/.local/bin"
fi

WORK_DIR="$WORK_ROOT/work"
mkdir -p "$WORK_DIR"

release_target="$(detect_target)"
ASSET_BASE_URL="${ASSET_BASE_URL_INPUT:-https://github.com/ugoite/ugoite/releases/download/${RELEASE_TAG_INPUT}}"
release_archive_name="ugoite-${RELEASE_TAG_INPUT}-${release_target}.tar.gz"
release_checksum_name="${release_archive_name}.sha256"
release_assets_dir="$WORK_ROOT/release-assets"
mkdir -p "$release_assets_dir"
download_asset "release-manifest.json" "$release_assets_dir/release-manifest.json"
download_asset "$release_archive_name" "$release_assets_dir/$release_archive_name"
download_asset "$release_checksum_name" "$release_assets_dir/$release_checksum_name"
verify_checksum "$release_assets_dir/$release_archive_name" "$release_assets_dir/$release_checksum_name"
validate_release_manifest \
  "$release_assets_dir/release-manifest.json" \
  "$release_assets_dir/$release_archive_name" \
  "$release_assets_dir/$release_checksum_name" \
  "$release_archive_name" \
  "$release_checksum_name"

export HOME="$QUICKSTART_HOME"
export PATH="$INSTALL_DIR:${PATH}"
export UGOITE_VERSION="$VERSION_INPUT"
export UGOITE_INSTALL_DIR="$INSTALL_DIR"

log "Installing ugoite ${VERSION_INPUT}"
UGOITE_DOWNLOAD_BASE_URL="$ASSET_BASE_URL" \
  UGOITE_RELEASE_TOKEN="$RELEASE_TOKEN_INPUT" \
  UGOITE_TARGET_OVERRIDE="$release_target" \
  /bin/bash "$INSTALL_SCRIPT_PATH"

INSTALLED_BINARY="$INSTALL_DIR/ugoite"
if [ ! -x "$INSTALLED_BINARY" ]; then
  fail "Expected installed binary at ${INSTALLED_BINARY}"
fi

help_output="$("$INSTALLED_BINARY" --help 2>&1)"
assert_help_output "$help_output"
log "Verified: ugoite --help"

version_output="$("$INSTALLED_BINARY" --version 2>&1)"
expected_version_output="ugoite ${VERSION_INPUT#v}"
if [ "$version_output" != "$expected_version_output" ]; then
  fail "installed binary reported ${version_output}, expected ${expected_version_output}"
fi
log "Verified: ugoite reports version ${VERSION_INPUT#v}"

mkdir -p "$WORK_DIR/data/spaces"

list_before_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space list "$SPACE_ROOT"
)"
assert_json_equals "initial space list" '[]' "$list_before_output"
log "Verified: space list starts empty"

create_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space create "${SPACE_ROOT}/${SPACE_ID}"
)"
assert_json_equals \
  "space create output" \
  "{\"created\": true, \"id\": \"${SPACE_ID}\"}" \
  "$create_output"
log "Verified: space create creates the expected demo space"

list_after_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space list "$SPACE_ROOT"
)"
assert_json_equals \
  "final space list" \
  "[\"${SPACE_ID}\"]" \
  "$list_after_output"
log "Verified: final space list contains the created space"

log "Quick-start smoke test passed for ${VERSION_INPUT}"
