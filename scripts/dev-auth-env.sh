#!/usr/bin/env bash
set -euo pipefail

if [ -z "${HOME:-}" ]; then
  echo "HOME is not set; cannot determine auth file path." >&2
  exit 1
fi

AUTH_MODE="${UGOITE_DEV_AUTH_MODE:-automatic}"
AUTH_FILE="${UGOITE_DEV_AUTH_FILE:-${HOME}/.ugoite/dev-auth.json}"
AUTH_TTL_SECONDS="${UGOITE_DEV_AUTH_TTL_SECONDS:-43200}"
DEV_2FA_SECRET="${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
DEV_USER_ID="${UGOITE_DEV_USER_ID:-dev-local-user}"

announce_mode() {
  local mode="$1"
  local detail="$2"
  echo "Local dev auth mode: ${mode} (${detail})" >&2
}

emit_exports() {
  local auth_token="$1"
  local bootstrap_token="$2"
  python - "$auth_token" "$bootstrap_token" "$DEV_USER_ID" <<'PY'
import shlex
import sys

auth_token = sys.argv[1]
bootstrap_token = sys.argv[2]
user_id = sys.argv[3]
print(f"export UGOITE_BOOTSTRAP_BEARER_TOKEN={shlex.quote(bootstrap_token)}")
print(f"export UGOITE_AUTH_BEARER_TOKEN={shlex.quote(auth_token)}")
print(f"export UGOITE_BOOTSTRAP_USER_ID={shlex.quote(user_id)}")
PY
}

derive_stable_token() {
  local prefix="$1"
  local seed="$2"
  python - "$prefix" "$DEV_USER_ID" "$seed" <<'PY'
import hashlib
import sys

prefix, user_id, seed = sys.argv[1:4]
digest = hashlib.sha256(f"{prefix}:{user_id}:{seed}".encode("utf-8")).hexdigest()[:32]
print(f"{prefix}-{digest}")
PY
}

resolve_existing_manual_token() {
  local auth_token="${UGOITE_AUTH_BEARER_TOKEN:-}"
  local bootstrap_token="${UGOITE_BOOTSTRAP_BEARER_TOKEN:-}"

  if [ -n "$auth_token" ] && [ -n "$bootstrap_token" ] && [ "$auth_token" != "$bootstrap_token" ]; then
    echo "Manual local dev auth requires UGOITE_AUTH_BEARER_TOKEN and UGOITE_BOOTSTRAP_BEARER_TOKEN to match." >&2
    exit 1
  fi

  if [ -n "$auth_token" ]; then
    printf '%s\n' "$auth_token"
    return 0
  fi

  if [ -n "$bootstrap_token" ]; then
    printf '%s\n' "$bootstrap_token"
    return 0
  fi

  return 1
}

automatic_auth_flow() {
  local auth_dir lock_file now_epoch token expires_at token_state
  auth_dir="$(dirname "$AUTH_FILE")"
  mkdir -p "$auth_dir"
  lock_file="${AUTH_FILE}.lock"
  exec 9>"$lock_file"
  if command -v flock >/dev/null 2>&1; then
    flock 9
  fi

  now_epoch="$(date +%s)"
  token=""
  expires_at="0"
  token_state="cached"

  if [ -f "$AUTH_FILE" ]; then
    read -r token expires_at < <(
      python - "$AUTH_FILE" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    payload = json.loads(path.read_text(encoding="utf-8"))
except Exception:
    print("", "0")
    raise SystemExit(0)

token = payload.get("bearer_token")
expires = payload.get("expires_at", 0)
if not isinstance(token, str):
    token = ""
if not isinstance(expires, int):
    expires = 0
print(token, expires)
PY
    )
  fi

  if [ -z "$token" ] || [ "$expires_at" -le "$now_epoch" ] || [ "${UGOITE_DEV_AUTH_FORCE_LOGIN:-false}" = "true" ]; then
    if ! command -v oathtool >/dev/null 2>&1; then
      echo "oathtool is required for automatic local dev 2FA flow. Install it first." >&2
      exit 1
    fi

    otp_code="$(oathtool --totp -b "$DEV_2FA_SECRET" | tr -d '\n')"
    if [ -z "$otp_code" ]; then
      echo "failed to generate 2FA code via oathtool" >&2
      exit 1
    fi

    token="$(
      python - <<'PY'
import secrets
print(f"dev-{secrets.token_urlsafe(24)}")
PY
    )"
    expires_at="$((now_epoch + AUTH_TTL_SECONDS))"
    token_state="generated"

    python - "$AUTH_FILE" "$token" "$expires_at" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
token = sys.argv[2]
expires_at = int(sys.argv[3])
path.parent.mkdir(parents=True, exist_ok=True)
payload = {
    "bearer_token": token,
    "expires_at": expires_at,
}
path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY
  fi

  announce_mode "automatic" "token=${token_state} auth_file=${AUTH_FILE}"
  emit_exports "$token" "$token"
}

manual_totp_flow() {
  local token token_source
  token=""
  token_source=""

  if [ -n "${UGOITE_DEV_MANUAL_TOKEN:-}" ]; then
    token="${UGOITE_DEV_MANUAL_TOKEN}"
    token_source="UGOITE_DEV_MANUAL_TOKEN"
  elif token="$(resolve_existing_manual_token)"; then
    token_source="pre-exported auth env"
  elif [ -n "${UGOITE_DEV_TOTP_CODE:-}" ]; then
    token="$(derive_stable_token "manual-totp" "${UGOITE_DEV_TOTP_CODE}")"
    token_source="UGOITE_DEV_TOTP_CODE"
  else
    cat >&2 <<'EOF'
manual-totp mode requires one of:
  - UGOITE_DEV_MANUAL_TOKEN=<token>
  - matching UGOITE_AUTH_BEARER_TOKEN / UGOITE_BOOTSTRAP_BEARER_TOKEN
  - UGOITE_DEV_TOTP_CODE="$(oathtool --totp -b "${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}")"
EOF
    exit 1
  fi

  announce_mode "manual-totp" "token_source=${token_source}"
  emit_exports "$token" "$token"
}

mock_oauth_flow() {
  local token token_source
  if [ -n "${UGOITE_DEV_MOCK_OAUTH_TOKEN:-}" ]; then
    token="${UGOITE_DEV_MOCK_OAUTH_TOKEN}"
    token_source="UGOITE_DEV_MOCK_OAUTH_TOKEN"
  else
    token="$(derive_stable_token "mock-oauth" "mock-oauth")"
    token_source="derived-from-user-id"
  fi

  announce_mode "mock-oauth" "token_source=${token_source}"
  emit_exports "$token" "$token"
}

case "$AUTH_MODE" in
  automatic)
    automatic_auth_flow
    ;;
  manual-totp)
    manual_totp_flow
    ;;
  mock-oauth)
    mock_oauth_flow
    ;;
  *)
    echo "Unsupported UGOITE_DEV_AUTH_MODE: ${AUTH_MODE}. Expected automatic, manual-totp, or mock-oauth." >&2
    exit 1
    ;;
esac
