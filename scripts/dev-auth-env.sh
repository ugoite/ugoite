#!/usr/bin/env bash
set -euo pipefail

if [ -z "${HOME:-}" ]; then
  echo "HOME is not set; cannot determine auth file path." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

AUTH_MODE="${UGOITE_DEV_AUTH_MODE:-passkey-totp}"
AUTH_FILE="${UGOITE_DEV_AUTH_FILE:-${HOME}/.ugoite/dev-auth.json}"
AUTH_TTL_SECONDS="${UGOITE_DEV_AUTH_TTL_SECONDS:-43200}"
DEV_2FA_SECRET="${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
DEV_USER_ID="${UGOITE_DEV_USER_ID:-}"
DEFAULT_DEV_USER_ID="dev-local-user"
DEV_SIGNING_KID="${UGOITE_DEV_SIGNING_KID:-dev-local-v1}"
DEV_PASSKEY_CONTEXT="${UGOITE_DEV_PASSKEY_CONTEXT:-}"

announce_mode() {
  local mode="$1"
  local detail="$2"
  echo "Local dev auth mode: ${mode} (${detail})" >&2
}

python_ugoite() {
  PYTHONPATH="${REPO_ROOT}/ugoite-core${PYTHONPATH:+:${PYTHONPATH}}" python "$@"
}

acquire_lock() {
  local auth_dir lock_file
  auth_dir="$(dirname "$AUTH_FILE")"
  mkdir -p "$auth_dir"
  lock_file="${AUTH_FILE}.lock"
  exec 9>"$lock_file"
  if command -v flock >/dev/null 2>&1; then
    flock 9
  fi
}

read_cached_context() {
  python_ugoite - "$AUTH_FILE" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
if not path.exists():
    raise SystemExit(0)

try:
    payload = json.loads(path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(0)

mode = payload.get("mode")
user_id = payload.get("user_id")
signing_secret = payload.get("signing_secret")
signing_kid = payload.get("signing_kid")
passkey_context = payload.get("passkey_context")

parts = [mode, user_id, signing_secret, signing_kid, passkey_context]
if not all(isinstance(part, str) and part for part in parts):
    raise SystemExit(0)

print("\t".join(parts))
PY
}

write_auth_context() {
  local mode="$1"
  local user_id="$2"
  local signing_secret="$3"
  local signing_kid="$4"
  local passkey_context="$5"
  python_ugoite - "$AUTH_FILE" "$mode" "$user_id" "$signing_secret" "$signing_kid" "$passkey_context" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
mode, user_id, signing_secret, signing_kid, passkey_context = sys.argv[2:7]
payload = {
    "mode": mode,
    "user_id": user_id,
    "signing_secret": signing_secret,
    "signing_kid": signing_kid,
    "passkey_context": passkey_context,
}
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
path.chmod(0o600)
PY
}

random_signing_secret() {
  python_ugoite - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
}

random_passkey_context() {
  python_ugoite - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
}

validate_totp_prompt_value() {
  local totp_code="$1"
  python_ugoite - "$totp_code" "$DEV_2FA_SECRET" <<'PY'
import sys

from ugoite_core.auth import validate_totp_code

code, secret = sys.argv[1:3]
raise SystemExit(0 if validate_totp_code(code, secret) else 1)
PY
}

prompt_non_empty() {
  local prompt_text="$1"
  local value=""
  while true; do
    read -r -p "$prompt_text" value
    value="$(printf '%s' "$value" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    [ -n "$value" ] && break
  done
  printf '%s\n' "$value"
}

emit_exports() {
  local mode="$1"
  local user_id="$2"
  local signing_secret="$3"
  local signing_kid="$4"
  local passkey_context="$5"
  local root_path="${UGOITE_ROOT:-$REPO_ROOT}"
  python_ugoite - "$mode" "$user_id" "$signing_secret" "$signing_kid" "$passkey_context" "$AUTH_TTL_SECONDS" "$DEV_2FA_SECRET" "$root_path" <<'PY'
import shlex
import sys

mode, user_id, signing_secret, signing_kid, passkey_context, ttl_seconds, dev_2fa_secret, root_path = sys.argv[1:9]
print("unset UGOITE_AUTH_BEARER_TOKEN")
print("unset UGOITE_BOOTSTRAP_BEARER_TOKEN")
print(f"export UGOITE_DEV_AUTH_MODE={shlex.quote(mode)}")
print(f"export UGOITE_DEV_USER_ID={shlex.quote(user_id)}")
print(f"export UGOITE_ROOT={shlex.quote(root_path)}")
print(f"export UGOITE_DEV_SIGNING_SECRET={shlex.quote(signing_secret)}")
print(f"export UGOITE_DEV_SIGNING_KID={shlex.quote(signing_kid)}")
print(f"export UGOITE_DEV_PASSKEY_CONTEXT={shlex.quote(passkey_context)}")
print(f"export UGOITE_DEV_AUTH_TTL_SECONDS={shlex.quote(ttl_seconds)}")
print(f"export UGOITE_DEV_2FA_SECRET={shlex.quote(dev_2fa_secret)}")
print(
    "export UGOITE_AUTH_BEARER_SECRETS="
    + shlex.quote(f"{signing_kid}:{signing_secret}")
)
print(f"export UGOITE_AUTH_BEARER_ACTIVE_KIDS={shlex.quote(signing_kid)}")
PY
}

reuse_or_create_context() {
  local requested_mode="$1"
  local entered_totp=""
  local cached_mode=""
  local cached_user_id=""
  local cached_signing_secret=""
  local cached_signing_kid=""
  local cached_passkey_context=""
  local cached_state=""

  acquire_lock
  cached_state="$(read_cached_context || true)"
  if [ -n "$cached_state" ]; then
    IFS=$'\t' read -r cached_mode cached_user_id cached_signing_secret cached_signing_kid cached_passkey_context <<<"$cached_state"
  fi

  if [ "${UGOITE_DEV_AUTH_FORCE_LOGIN:-false}" != "true" ] &&
    [ "$requested_mode" = "$cached_mode" ] &&
    { [ -z "$DEV_USER_ID" ] || [ "$DEV_USER_ID" = "$cached_user_id" ]; } &&
    [ -n "$cached_signing_secret" ] &&
    [ -n "$cached_signing_kid" ] &&
    [ -n "$cached_passkey_context" ]; then
    emit_exports "$requested_mode" "$cached_user_id" "$cached_signing_secret" "$cached_signing_kid" "$cached_passkey_context"
    echo "Using cached local dev auth context for ${cached_user_id}." >&2
    return 0
  fi

  if [ "$requested_mode" = "passkey-totp" ]; then
    if [ -z "$DEV_USER_ID" ]; then
      DEV_USER_ID="$(prompt_non_empty 'Local dev username: ')"
    fi
    entered_totp="${UGOITE_DEV_TOTP_CODE:-}"
    if [ -z "$entered_totp" ]; then
      entered_totp="$(prompt_non_empty 'Current 2FA code: ')"
    fi
    if ! validate_totp_prompt_value "$entered_totp"; then
      echo "The provided 2FA code is invalid for UGOITE_DEV_2FA_SECRET." >&2
      exit 1
    fi
  elif [ -z "$DEV_USER_ID" ]; then
    DEV_USER_ID="$DEFAULT_DEV_USER_ID"
  fi

  local signing_secret signing_kid
  local passkey_context
  signing_secret="${UGOITE_DEV_SIGNING_SECRET:-}"
  if [ -z "$signing_secret" ]; then
    signing_secret="$(random_signing_secret)"
  fi
  signing_kid="$DEV_SIGNING_KID"
  passkey_context="$DEV_PASSKEY_CONTEXT"
  if [ -z "$passkey_context" ]; then
    passkey_context="$(random_passkey_context)"
  fi

  write_auth_context "$requested_mode" "$DEV_USER_ID" "$signing_secret" "$signing_kid" "$passkey_context"
  emit_exports "$requested_mode" "$DEV_USER_ID" "$signing_secret" "$signing_kid" "$passkey_context"
  echo "Prepared local dev auth context for ${DEV_USER_ID}." >&2
}

case "$AUTH_MODE" in
  passkey-totp)
    announce_mode "passkey-totp" "passkey-bound username + 2FA login"
    reuse_or_create_context "passkey-totp"
    ;;
  mock-oauth)
    announce_mode "mock-oauth" "explicit browser or CLI login"
    reuse_or_create_context "mock-oauth"
    ;;
  *)
    echo "Unsupported UGOITE_DEV_AUTH_MODE: ${AUTH_MODE}. Expected passkey-totp or mock-oauth." >&2
    exit 1
    ;;
esac
