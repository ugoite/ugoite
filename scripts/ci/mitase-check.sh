#!/usr/bin/env bash
set -euo pipefail

readonly MITASE_RELEASE_TAG="v0.1.1"
readonly MITASE_SOURCE_SHA="4406e99dc6df7a10268104bf2bbc5e7ba45aacf7"
readonly MITASE_CANDIDATE_ID="sha256:80ded900034169238c623de57c1e43676ab0bb66f22fe84195bf5bb80652b1c2"
readonly MITASE_MANIFEST_SHA256="e3c628c8a501702021d284d0d61949bb99a7230574c1c2ff45c0d5b16506ec9d"
MITASE_RELEASE_BASE_URL="${MITASE_RELEASE_BASE_URL:-https://github.com/ugoite/mitase/releases/download/${MITASE_RELEASE_TAG}}"

if [[ -n "${MITASE_BIN:-}" ]]; then
  exec "$MITASE_BIN" check .
fi

case "$(uname -s):$(uname -m)" in
  Darwin:arm64|Darwin:aarch64)
    MITASE_RELEASE_TARGET="aarch64-apple-darwin"
    MITASE_ARCHIVE_SHA256="3cc4ca01a6a6c984182919da2d8f7880fa947366c47a786e4ddaa63f86dad803"
    ;;
  Darwin:x86_64)
    MITASE_RELEASE_TARGET="x86_64-apple-darwin"
    MITASE_ARCHIVE_SHA256="86f671f532b92efc45ae42dcc7ac68c2a1d3cbfbd6ea65ac738e69fd5ee0ad09"
    ;;
  Linux:aarch64|Linux:arm64)
    MITASE_RELEASE_TARGET="aarch64-unknown-linux-gnu"
    MITASE_ARCHIVE_SHA256="44a5afce35c48bea69fc446028a2203d8a51244df5b4a385b67a4964f742edaf"
    ;;
  Linux:x86_64)
    MITASE_RELEASE_TARGET="x86_64-unknown-linux-gnu"
    MITASE_ARCHIVE_SHA256="11103de15e91c656a2bbe24622ead88fa69b6b51d8b691683fedd035e99e7eaf"
    ;;
  *)
    printf 'Unsupported Mitase release target; set MITASE_BIN for local development\n' >&2
    exit 1
    ;;
esac

readonly MITASE_RELEASE_TARGET
readonly MITASE_ARCHIVE_SHA256
MITASE_ROOT="${MITASE_ROOT:-target/mitase-${MITASE_RELEASE_TAG}-${MITASE_RELEASE_TARGET}}"

sha256_file() {
  local path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  else
    printf 'Mitase check requires sha256sum or shasum\n' >&2
    return 1
  fi
}

verify_sha256() {
  local path="$1"
  local expected="$2"
  local actual

  actual="$(sha256_file "$path")"
  if [[ "$actual" != "$expected" ]]; then
    printf 'Mitase release artifact checksum mismatch for %s\nexpected: %s\nactual:   %s\n' \
      "$path" "$expected" "$actual" >&2
    return 1
  fi
}

for command in curl tar install; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Mitase check requires %s\n' "$command" >&2
    exit 1
  fi
done

MITASE_TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mitase-check.XXXXXX")"
trap 'rm -rf "$MITASE_TEMP_DIR"' EXIT

MITASE_MANIFEST_PATH="${MITASE_TEMP_DIR}/candidate-manifest.json"
MITASE_ARCHIVE_NAME="mitase-${MITASE_RELEASE_TARGET}.tar.gz"
MITASE_ARCHIVE_PATH="${MITASE_TEMP_DIR}/${MITASE_ARCHIVE_NAME}"

curl --fail --location --silent --show-error --retry 3 \
  "${MITASE_RELEASE_BASE_URL}/candidate-manifest.json" \
  --output "$MITASE_MANIFEST_PATH"
verify_sha256 "$MITASE_MANIFEST_PATH" "$MITASE_MANIFEST_SHA256"

if ! grep -Fq "\"candidate_id\":\"$MITASE_CANDIDATE_ID\"" "$MITASE_MANIFEST_PATH" || \
  ! grep -Fq "\"source_sha\":\"$MITASE_SOURCE_SHA\"" "$MITASE_MANIFEST_PATH"; then
  printf 'Mitase candidate manifest identity mismatch\n' >&2
  exit 1
fi

curl --fail --location --silent --show-error --retry 3 \
  "${MITASE_RELEASE_BASE_URL}/${MITASE_ARCHIVE_NAME}" \
  --output "$MITASE_ARCHIVE_PATH"
verify_sha256 "$MITASE_ARCHIVE_PATH" "$MITASE_ARCHIVE_SHA256"

printf 'Using Mitase %s candidate %s from source %s\n' \
  "$MITASE_RELEASE_TAG" "$MITASE_CANDIDATE_ID" "$MITASE_SOURCE_SHA"

MITASE_EXTRACT_DIR="${MITASE_TEMP_DIR}/extracted"
mkdir -p "$MITASE_EXTRACT_DIR"
tar --extract --gzip --file "$MITASE_ARCHIVE_PATH" --directory "$MITASE_EXTRACT_DIR"

if [[ ! -x "${MITASE_EXTRACT_DIR}/mitase" ]]; then
  printf 'Mitase release archive does not contain an executable mitase binary\n' >&2
  exit 1
fi

mkdir -p "${MITASE_ROOT}/bin"
install -m 0755 "${MITASE_EXTRACT_DIR}/mitase" "${MITASE_ROOT}/bin/mitase"

exec "${MITASE_ROOT}/bin/mitase" check .
