#!/usr/bin/env bash
set -euo pipefail

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

if [ "$#" -ne 2 ]; then
  fail "usage: $0 <org> <package-name>"
fi

org="$1"
package_name="$2"
token="${GHCR_VISIBILITY_TOKEN:-}"

if [ -z "$token" ]; then
  fail "GHCR_VISIBILITY_TOKEN must be set"
fi

require_command curl
require_command python3

encoded_package="$(
  python3 - "$package_name" <<'PY'
import sys
import urllib.parse

print(urllib.parse.quote(sys.argv[1], safe=""))
PY
)"

response_file="$(mktemp)"
cleanup() {
  rm -f "$response_file"
}
trap cleanup EXIT HUP INT TERM

request() {
  local method="$1"
  local url="$2"
  local data="${3:-}"
  local curl_args=(
    -sS
    -o "$response_file"
    -w "%{http_code}"
    -X "$method"
    -H "Accept: application/vnd.github+json"
    -H "Authorization: Bearer $token"
    -H "X-GitHub-Api-Version: 2022-11-28"
    "$url"
  )

  if [ -n "$data" ]; then
    curl_args+=(
      -H "Content-Type: application/json"
      --data "$data"
    )
  fi

  curl "${curl_args[@]}"
}

package_url="https://api.github.com/orgs/${org}/packages/container/${encoded_package}"
visibility_url="${package_url}/visibility"

status_code=""
for attempt in {1..20}; do
  status_code="$(request GET "$package_url")"
  if [ "$status_code" = "200" ]; then
    break
  fi
  if [ "$attempt" -lt 20 ]; then
    sleep 3
  fi
done

if [ "$status_code" != "200" ]; then
  log "GitHub package lookup failed for ${org}/${package_name} (HTTP ${status_code})"
  cat "$response_file" >&2
  exit 1
fi

if python3 - "$response_file" <<'PY'; then
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    payload = json.load(fh)

raise SystemExit(0 if payload.get("visibility") == "public" else 1)
PY
  log "GHCR package ${org}/${package_name} is already public."
  exit 0
fi

status_code="$(request PATCH "$visibility_url" '{"visibility":"public"}')"

case "$status_code" in
  200 | 201 | 202 | 204)
    log "Marked GHCR package ${org}/${package_name} as public."
    ;;
  *)
    log "Failed to mark GHCR package ${org}/${package_name} as public (HTTP ${status_code})"
    cat "$response_file" >&2
    exit 1
    ;;
esac
