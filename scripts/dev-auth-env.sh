#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

AUTH_MODE="${UGOITE_DEV_AUTH_MODE:-mock-oauth}"
DEV_USER_ID="${UGOITE_DEV_USER_ID:-dev-local-user}"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_SIGNING_SECRET="${UGOITE_DEV_SIGNING_SECRET:-}"
BOOTSTRAP_TOKEN="${UGOITE_BOOTSTRAP_TOKEN:-dev-token}"
ROOT_PATH="${UGOITE_ROOT:-$REPO_ROOT}"

quote_shell() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

random_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -base64 32 | tr -d '\n'
    return
  fi
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr -d '\n'
    uuidgen | tr -d '\n'
    return
  fi
  echo "dev-local-signing-secret-change-me"
}

if [ "$AUTH_MODE" != "mock-oauth" ]; then
  echo "Unsupported UGOITE_DEV_AUTH_MODE: ${AUTH_MODE}." >&2
  echo "The current Rust server release ships mock-oauth for local dev; passkey-totp is planned but not implemented end to end." >&2
  exit 1
fi

if [ -z "$DEV_SIGNING_SECRET" ]; then
  DEV_SIGNING_SECRET="$(random_secret)"
fi

cat <<EOF
unset UGOITE_AUTH_BEARER_TOKEN
unset UGOITE_BOOTSTRAP_BEARER_TOKEN
export UGOITE_DEV_AUTH_MODE=$(quote_shell "$AUTH_MODE")
export UGOITE_DEV_USER_ID=$(quote_shell "$DEV_USER_ID")
export UGOITE_ROOT=$(quote_shell "$ROOT_PATH")
export UGOITE_BOOTSTRAP_TOKEN=$(quote_shell "$BOOTSTRAP_TOKEN")
export UGOITE_DEV_SIGNING_KID=$(quote_shell "$DEV_SIGNING_KID")
export UGOITE_DEV_SIGNING_SECRET=$(quote_shell "$DEV_SIGNING_SECRET")
export UGOITE_AUTH_BEARER_SIGNING_SECRETS=$(quote_shell "${DEV_SIGNING_KID}:${DEV_SIGNING_SECRET}")
export UGOITE_AUTH_BEARER_ACTIVE_KIDS=$(quote_shell "$DEV_SIGNING_KID")
EOF

echo "Prepared local dev mock-oauth context for ${DEV_USER_ID}." >&2
