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
SPACE_ROOT="./spaces"

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

  python3 - "$label" "$expected_json" "$actual_json" <<'PY'
import json
import sys

label, expected_raw, actual_raw = sys.argv[1:4]

try:
    expected = json.loads(expected_raw)
except json.JSONDecodeError as exc:
    print(
        f"{label}: expected JSON fixture could not be decoded: {exc}: {expected_raw!r}",
        file=sys.stderr,
    )
    sys.exit(1)

try:
    actual = json.loads(actual_raw)
except json.JSONDecodeError as exc:
    print(
        f"{label}: command output was not valid JSON: {exc}: {actual_raw!r}",
        file=sys.stderr,
    )
    sys.exit(1)

if actual != expected:
    print(
        f"{label}: expected {expected!r} but got {actual!r}",
        file=sys.stderr,
    )
    sys.exit(1)
PY
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

require_command python3

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

mkdir -p "$WORK_DIR/spaces"

list_before_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space list --root "$SPACE_ROOT"
)"
assert_json_equals "initial space list" '[]' "$list_before_output"
log "Verified: space list starts empty"

create_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space create --root "$SPACE_ROOT" "$SPACE_ID"
)"
assert_json_equals \
  "space create output" \
  "{\"created\": true, \"id\": \"${SPACE_ID}\"}" \
  "$create_output"
log "Verified: space create creates the expected demo space"

list_after_output="$(
  cd "$WORK_DIR" &&
    "$INSTALLED_BINARY" space list --root "$SPACE_ROOT"
)"
assert_json_equals \
  "final space list" \
  "[\"${SPACE_ID}\"]" \
  "$list_after_output"
log "Verified: final space list contains the created space"

log "Quick-start smoke test passed for ${VERSION_INPUT}"
