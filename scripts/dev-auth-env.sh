#!/usr/bin/env bash
set -euo pipefail

if [ -z "${HOME:-}" ]; then
  echo "HOME is not set; cannot determine auth file path." >&2
  exit 1
fi
AUTH_FILE="${UGOITE_DEV_AUTH_FILE:-${HOME}/.ugoite/dev-auth.json}"
AUTH_TTL_SECONDS="${UGOITE_DEV_AUTH_TTL_SECONDS:-43200}"
DEV_2FA_SECRET="${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
DEV_USER_ID="${UGOITE_DEV_USER_ID:-dev-local-user}"

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
    echo "oathtool is required for local dev 2FA flow. Install it first." >&2
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

  echo "Generated new local dev token; expires_at=${expires_at}" >&2
fi

python - "$token" "$DEV_USER_ID" <<'PY'
import shlex
import sys

token = sys.argv[1]
user_id = sys.argv[2]
print(f"export UGOITE_BOOTSTRAP_BEARER_TOKEN={shlex.quote(token)}")
print(f"export UGOITE_AUTH_BEARER_TOKEN={shlex.quote(token)}")
print(f"export UGOITE_BOOTSTRAP_USER_ID={shlex.quote(user_id)}")
PY
