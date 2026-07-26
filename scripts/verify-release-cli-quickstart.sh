#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_SCRIPT_PATH="${UGOITE_INSTALL_SCRIPT_PATH:-${SCRIPT_DIR}/install-ugoite-cli.sh}"
VERSION_INPUT="${UGOITE_VERSION:-}"
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

export HOME="$QUICKSTART_HOME"
export PATH="$INSTALL_DIR:${PATH}"
export UGOITE_VERSION="$VERSION_INPUT"
export UGOITE_INSTALL_DIR="$INSTALL_DIR"

log "Installing ugoite ${VERSION_INPUT}"
/bin/bash "$INSTALL_SCRIPT_PATH"

INSTALLED_BINARY="$INSTALL_DIR/ugoite"
if [ ! -x "$INSTALLED_BINARY" ]; then
  fail "Expected installed binary at ${INSTALLED_BINARY}"
fi

help_output="$("$INSTALLED_BINARY" --help 2>&1)"
assert_help_output "$help_output"
log "Verified: ugoite --help"

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
