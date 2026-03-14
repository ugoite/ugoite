#!/usr/bin/env bash
set -euo pipefail

REPO="${UGOITE_GITHUB_REPO:-ugoite/ugoite}"
VERSION_INPUT="${UGOITE_VERSION:-latest}"
INSTALL_DIR="${UGOITE_INSTALL_DIR:-$HOME/.local/bin}"
DOWNLOAD_BASE_URL="${UGOITE_DOWNLOAD_BASE_URL:-}"

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

resolve_tag() {
  if [ -n "$DOWNLOAD_BASE_URL" ]; then
    case "$VERSION_INPUT" in
      latest)
        fail "UGOITE_VERSION must be set to an exact release when UGOITE_DOWNLOAD_BASE_URL is used"
        ;;
      v*)
        printf '%s' "$VERSION_INPUT"
        ;;
      *)
        printf 'v%s' "$VERSION_INPUT"
        ;;
    esac
    return
  fi

  if [ "$VERSION_INPUT" = "latest" ]; then
    tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^\"]*\)".*/\1/p' | head -n 1)"
    [ -n "$tag" ] || fail "Could not resolve the latest Ugoite release tag"
    printf '%s' "$tag"
    return
  fi

  case "$VERSION_INPUT" in
    v*) printf '%s' "$VERSION_INPUT" ;;
    *) printf 'v%s' "$VERSION_INPUT" ;;
  esac
}

detect_target() {
  os_name="$(uname -s)"
  arch_name="$(uname -m)"

  case "$os_name" in
    Linux)
      case "$arch_name" in
        x86_64) printf '%s' 'x86_64-unknown-linux-gnu' ;;
        arm64 | aarch64) printf '%s' 'aarch64-unknown-linux-gnu' ;;
        *) fail "Unsupported Linux architecture: $arch_name" ;;
      esac
      ;;
    Darwin)
      case "$arch_name" in
        x86_64) printf '%s' 'x86_64-apple-darwin' ;;
        arm64 | aarch64) printf '%s' 'aarch64-apple-darwin' ;;
        *) fail "Unsupported macOS architecture: $arch_name" ;;
      esac
      ;;
    *)
      fail "Unsupported operating system: $os_name"
      ;;
  esac
}

verify_checksum() {
  archive_path="$1"
  checksum_path="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "$archive_path")" && sha256sum -c "$(basename "$checksum_path")")
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    expected="$(cut -d ' ' -f 1 <"$checksum_path")"
    actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
    [ "$expected" = "$actual" ] || fail "Checksum verification failed for $(basename "$archive_path")"
    return
  fi

  fail "Need sha256sum or shasum to verify the downloaded archive"
}

require_command curl
require_command tar
require_command install

release_tag="$(resolve_tag)"
version_no_v="${release_tag#v}"
release_target="$(detect_target)"
asset_name="ugoite-${release_tag}-${release_target}.tar.gz"
checksum_name="${asset_name}.sha256"

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT HUP INT TERM

if [ -n "$DOWNLOAD_BASE_URL" ]; then
  asset_base_url="${DOWNLOAD_BASE_URL%/}"
else
  asset_base_url="https://github.com/${REPO}/releases/download/${release_tag}"
fi

log "Downloading ugoite ${version_no_v} for ${release_target}"
curl -fsSL "${asset_base_url}/${asset_name}" -o "${tmpdir}/${asset_name}"
curl -fsSL "${asset_base_url}/${checksum_name}" -o "${tmpdir}/${checksum_name}"
verify_checksum "${tmpdir}/${asset_name}" "${tmpdir}/${checksum_name}"

mkdir -p "$INSTALL_DIR"
tar -xzf "${tmpdir}/${asset_name}" -C "$tmpdir"
install -m 0755 "${tmpdir}/ugoite" "${INSTALL_DIR}/ugoite"

log "Installed ugoite to ${INSTALL_DIR}/ugoite"
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    log "Add ${INSTALL_DIR} to PATH if it is not already available in your shell."
    ;;
esac
