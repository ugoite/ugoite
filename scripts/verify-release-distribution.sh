#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION_INPUT="${UGOITE_VERSION:-}"
RELEASE_TAG_INPUT="${UGOITE_RELEASE_TAG:-v${VERSION_INPUT}}"
RELEASE_SHA_INPUT="${UGOITE_RELEASE_SHA:-}"
CANDIDATE_ID_INPUT="${UGOITE_CANDIDATE_ID:-}"
IMAGE_REPOSITORY="${UGOITE_IMAGE_REPOSITORY:-ghcr.io/ugoite/ugoite}"
RELEASE_REPOSITORY_INPUT="${UGOITE_RELEASE_REPOSITORY:-ugoite/ugoite}"
RELEASE_TOKEN_INPUT="${UGOITE_RELEASE_TOKEN:-}"
ASSET_BASE_URL_INPUT="${UGOITE_RELEASE_ASSET_BASE_URL:-}"
INSTALL_DIR_INPUT="${UGOITE_INSTALL_DIR:-}"
VERIFIER_WORKFLOW_SHA_INPUT="${UGOITE_VERIFIER_WORKFLOW_SHA:-}"
VERIFICATION_RUN_ID_INPUT="${UGOITE_VERIFICATION_RUN_ID:-${GITHUB_RUN_ID:-}}"

if [ -n "$RELEASE_TOKEN_INPUT" ] && [ -z "${GH_TOKEN:-}" ]; then
  export GH_TOKEN="$RELEASE_TOKEN_INPUT"
fi

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
  local attempt
  local -a curl_args=(-fsSL)

  if [ -n "$RELEASE_TOKEN_INPUT" ]; then
    curl_args+=(
      -H "Authorization: Bearer ${RELEASE_TOKEN_INPUT}"
      -H "Accept: application/octet-stream"
    )
  fi

  for attempt in $(seq 1 10); do
    if curl "${curl_args[@]}" -o "$output_path" "${ASSET_BASE_URL}/${asset_name}"; then
      return 0
    fi
    if [ "$attempt" -eq 10 ]; then
      fail "Failed to download ${asset_name} after ${attempt} attempts"
    fi
    sleep 3
  done
}

detect_target() {
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64) printf '%s' 'x86_64-unknown-linux-gnu' ;;
    Linux:arm64 | Linux:aarch64) printf '%s' 'aarch64-unknown-linux-gnu' ;;
    Darwin:x86_64) printf '%s' 'x86_64-apple-darwin' ;;
    Darwin:arm64 | Darwin:aarch64) printf '%s' 'aarch64-apple-darwin' ;;
    *) fail "Unsupported release CLI target: $(uname -s) $(uname -m)" ;;
  esac
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

verify_checksum() {
  local archive_path="$1"
  local checksum_path="$2"
  local expected actual
  expected="$(awk '{print $1}' <"$checksum_path")"
  actual="$(sha256_file "$archive_path")"
  [ "$expected" = "$actual" ] || fail "Checksum verification failed for $(basename "$archive_path")"
}

if [ -z "$VERSION_INPUT" ] || [ -z "$RELEASE_SHA_INPUT" ] || [ -z "$CANDIDATE_ID_INPUT" ]; then
  fail "UGOITE_VERSION, UGOITE_RELEASE_SHA, and UGOITE_CANDIDATE_ID are required"
fi

require_command curl
require_command deno
require_command docker
require_command gh
require_command helm
require_command npm
require_command tar

cd "$REPO_ROOT"

ASSET_BASE_URL="${ASSET_BASE_URL_INPUT:-https://github.com/${RELEASE_REPOSITORY_INPUT}/releases/download/${RELEASE_TAG_INPUT}}"
WORK_ROOT="$(mktemp -d)"
INSTALL_DIR="${INSTALL_DIR_INPUT:-$WORK_ROOT/bin}"
CLI_TARGET="$(detect_target)"
CLI_ARCHIVE="ugoite-${RELEASE_TAG_INPUT}-${CLI_TARGET}.tar.gz"
CLI_CHECKSUM="${CLI_ARCHIVE}.sha256"
CONTAINER_NAME="ugoite-distribution-${RANDOM}-${RANDOM}"
CONTAINER_STARTED=0

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [ "$CONTAINER_STARTED" -eq 1 ]; then
    docker rm --force "$CONTAINER_NAME" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK_ROOT"
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$WORK_ROOT/assets"
receipt_asset_name="$(gh release view "$RELEASE_TAG_INPUT" --repo "$RELEASE_REPOSITORY_INPUT" --json assets --jq '.assets[].name' | awk '/^verification-receipt-[^/]+\.json$/ { print; exit }')"
[ -n "$receipt_asset_name" ] || fail "published release has no verification receipt asset"
immutable="$(gh release view "$RELEASE_TAG_INPUT" --repo "$RELEASE_REPOSITORY_INPUT" --json isImmutable --jq '.isImmutable')"
[ "$immutable" = "true" ] || fail "published release is not immutable"
download_asset candidate-manifest.json "$WORK_ROOT/assets/candidate-manifest.json"
download_asset candidate-id.txt "$WORK_ROOT/assets/candidate-id.txt"
download_asset release-manifest.json "$WORK_ROOT/assets/release-manifest.json"
download_asset "$receipt_asset_name" "$WORK_ROOT/assets/$receipt_asset_name"
download_asset docker-compose.release.yaml "$WORK_ROOT/assets/docker-compose.release.yaml"
download_asset docker-compose.release.yaml.sha256 "$WORK_ROOT/assets/docker-compose.release.yaml.sha256"
download_asset "$CLI_ARCHIVE" "$WORK_ROOT/assets/$CLI_ARCHIVE"
download_asset "$CLI_CHECKSUM" "$WORK_ROOT/assets/$CLI_CHECKSUM"
verify_checksum "$WORK_ROOT/assets/docker-compose.release.yaml" \
  "$WORK_ROOT/assets/docker-compose.release.yaml.sha256"
verify_checksum "$WORK_ROOT/assets/$CLI_ARCHIVE" "$WORK_ROOT/assets/$CLI_CHECKSUM"

export MANIFEST_PATH="$WORK_ROOT/assets/release-manifest.json"
export CANDIDATE_MANIFEST_PATH="$WORK_ROOT/assets/candidate-manifest.json"
export CANDIDATE_ID_PATH="$WORK_ROOT/assets/candidate-id.txt"
export VERIFICATION_RECEIPT_PATH="$WORK_ROOT/assets/$receipt_asset_name"
export COMPOSE_PATH="$WORK_ROOT/assets/docker-compose.release.yaml"
export COMPOSE_CHECKSUM_PATH="$WORK_ROOT/assets/docker-compose.release.yaml.sha256"
export CLI_ARCHIVE_PATH="$WORK_ROOT/assets/$CLI_ARCHIVE"
export CLI_ARCHIVE_NAME="$CLI_ARCHIVE"
export RELEASE_TAG_INPUT VERSION_INPUT RELEASE_SHA_INPUT IMAGE_REPOSITORY CANDIDATE_ID_INPUT
# Stable distribution verification lives in tools/distribution.ts; the shell
# keeps only Docker/curl/registry orchestration around these calls.
verify_manifest_args=(
  verify-manifest
  --manifest "$MANIFEST_PATH"
  --candidate-manifest "$CANDIDATE_MANIFEST_PATH"
  --candidate-id-path "$CANDIDATE_ID_PATH"
  --receipt "$VERIFICATION_RECEIPT_PATH"
  --compose-path "$COMPOSE_PATH"
  --compose-checksum-path "$COMPOSE_CHECKSUM_PATH"
  --cli-archive-path "$CLI_ARCHIVE_PATH"
  --cli-archive-name "$CLI_ARCHIVE_NAME"
  --release-tag "$RELEASE_TAG_INPUT"
  --version "$VERSION_INPUT"
  --source-sha "$RELEASE_SHA_INPUT"
  --image-repository "$IMAGE_REPOSITORY"
  --candidate-id "$CANDIDATE_ID_INPUT"
)
if [ -n "$VERIFIER_WORKFLOW_SHA_INPUT" ]; then
  verify_manifest_args+=(--verifier-workflow-sha "$VERIFIER_WORKFLOW_SHA_INPUT")
fi
if [ -n "$VERIFICATION_RUN_ID_INPUT" ]; then
  verify_manifest_args+=(--verification-run-id "$VERIFICATION_RUN_ID_INPUT")
fi
deno run -A tools/distribution.ts "${verify_manifest_args[@]}"

log "Verifying all published release assets"
asset_names="$(deno run -A tools/distribution.ts list-files --manifest "$MANIFEST_PATH")"
while IFS= read -r asset_name; do
  [ -n "$asset_name" ] || continue
  asset_path="$WORK_ROOT/assets/$asset_name"
  mkdir -p "$(dirname "$asset_path")"
  if [ ! -f "$asset_path" ]; then
    download_asset "$asset_name" "$asset_path"
  fi
  deno run -A tools/distribution.ts verify-file --manifest "$MANIFEST_PATH" --name "$asset_name" --path "$asset_path"
done <<<"$asset_names"

log "Verifying complete release asset set (no missing or unexpected assets)"
published_asset_names="$(gh release view "$RELEASE_TAG_INPUT" --repo "$RELEASE_REPOSITORY_INPUT" --json assets --jq '.assets[].name')"
published_asset_file="$WORK_ROOT/published-asset-names.txt"
printf '%s\n' "$published_asset_names" >"$published_asset_file"
deno run -A tools/distribution.ts check-asset-set --manifest "$MANIFEST_PATH" --assets "$published_asset_file"

expected_npm_sha="$(deno run -A tools/distribution.ts manifest-digest --manifest "$MANIFEST_PATH" --field npm)"
npm_url="$(npm view "@ugoite/ugoite@${VERSION_INPUT}" dist.tarball --json | tr -d '"')"
npm_path="$WORK_ROOT/npm.tgz"
declare -a npm_curl_args=(-fsSL)
if [ -n "${NODE_AUTH_TOKEN:-}" ]; then
  npm_curl_args+=(
    -H "Authorization: Bearer ${NODE_AUTH_TOKEN}"
    -H "Accept: application/octet-stream"
  )
fi
curl "${npm_curl_args[@]}" "$npm_url" -o "$npm_path"
[ "$(sha256_file "$npm_path")" = "$expected_npm_sha" ] || fail "Published npm package differs from candidate"

helm_digest="$(deno run -A tools/distribution.ts manifest-digest --manifest "$MANIFEST_PATH" --field helm)"
helm_dir="$WORK_ROOT/helm"
mkdir -p "$helm_dir"
helm pull oci://ghcr.io/ugoite/charts/ugoite --version "$VERSION_INPUT" --destination "$helm_dir" >/dev/null
helm_path="$helm_dir/ugoite-${VERSION_INPUT}.tgz"
[ "$(sha256_file "$helm_path")" = "$helm_digest" ] || fail "Published Helm chart differs from candidate"
log "Verified published npm and Helm artifacts against the candidate"

log "Verifying published image digest and health"
EXPECTED_IMAGE_DIGEST="$(deno run -A tools/distribution.ts manifest-digest --manifest "$MANIFEST_PATH" --field image)"
actual_image_digest="$(docker buildx imagetools inspect "${IMAGE_REPOSITORY}:${VERSION_INPUT}" --format '{{json .Manifest.Digest}}' | tr -d '"')"
[ "$actual_image_digest" = "$EXPECTED_IMAGE_DIGEST" ] || fail "version tag points to ${actual_image_digest}, expected ${EXPECTED_IMAGE_DIGEST}"

node_secret="$(head -c 32 /dev/urandom | base64 | tr -d '\n')"
docker run --detach --rm --name "$CONTAINER_NAME" \
  --publish 127.0.0.1::8000 \
  --env UGOITE_ROOT=/data \
  --env UGOITE_SERVER_ADDRESS=0.0.0.0:8000 \
  --env UGOITE_PUBLIC_ORIGIN=http://localhost \
  --env UGOITE_API_BASE_URL=http://localhost/api \
  --env UGOITE_WEBAUTHN_RP_ID=localhost \
  --env "UGOITE_NODE_SECRET_KEY=${node_secret}" \
  "${IMAGE_REPOSITORY}@${EXPECTED_IMAGE_DIGEST}" >/dev/null
CONTAINER_STARTED=1
container_port="$(docker port "$CONTAINER_NAME" 8000/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p')"
[ -n "$container_port" ] || fail "published container did not expose port 8000"
for attempt in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:${container_port}/health" >/dev/null; then
    break
  fi
  if [ "$attempt" -eq 60 ]; then
    fail "published container did not become healthy"
  fi
  sleep 1
done

log "Verifying published CLI installer and version"
mkdir -p "$INSTALL_DIR"
HOME="$WORK_ROOT/home" PATH="$INSTALL_DIR:$PATH" UGOITE_VERSION="$VERSION_INPUT" \
  UGOITE_INSTALL_DIR="$INSTALL_DIR" UGOITE_DOWNLOAD_BASE_URL="$ASSET_BASE_URL" \
  UGOITE_RELEASE_TOKEN="$RELEASE_TOKEN_INPUT" UGOITE_TARGET_OVERRIDE="$CLI_TARGET" \
  /bin/bash "$SCRIPT_DIR/install-ugoite-cli.sh"
version_output="$($INSTALL_DIR/ugoite --version 2>&1)"
[ "$version_output" = "ugoite ${VERSION_INPUT#v}" ] || \
  fail "published CLI reported ${version_output}, expected ugoite ${VERSION_INPUT#v}"

log "Published distribution verification passed for ${VERSION_INPUT}"
