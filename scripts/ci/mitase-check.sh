#!/usr/bin/env bash
set -euo pipefail

readonly MITASE_RELEASE_TAG="v0.1.0"
readonly MITASE_SOURCE_SHA="d0bfa043c7d4305b1a604432d2f97419db0dbb5c"
readonly MITASE_CANDIDATE_ID="sha256:771619840a3547fc167a5a98fbff3462d79b997abd2941935d6ced573eb9d82a"
readonly MITASE_MANIFEST_SHA256="dcae7fe842550efba70d99efbdfa12ba87d0263af6f98763fb22330b194b7c66"
MITASE_RELEASE_BASE_URL="${MITASE_RELEASE_BASE_URL:-https://github.com/ugoite/mitase/releases/download/${MITASE_RELEASE_TAG}}"

if [[ -n "${MITASE_BIN:-}" ]]; then
  exec "$MITASE_BIN" check .
fi

case "$(uname -s):$(uname -m)" in
  Darwin:arm64|Darwin:aarch64)
    MITASE_RELEASE_TARGET="aarch64-apple-darwin"
    MITASE_ARCHIVE_SHA256="9701167c9f3dbdd545dc9c3c21f1b43db8d6c6548a0bf12c66ee0607f20b0a62"
    ;;
  Darwin:x86_64)
    MITASE_RELEASE_TARGET="x86_64-apple-darwin"
    MITASE_ARCHIVE_SHA256="927b8afc9f55781693434af876e430790c0159a61f3a639a7865ccd401fa68cc"
    ;;
  Linux:aarch64|Linux:arm64)
    MITASE_RELEASE_TARGET="aarch64-unknown-linux-gnu"
    MITASE_ARCHIVE_SHA256="20e063b9243e4061a0aeb6cccfadfb8e9f52c55c85599f9d7739ad004ca153e3"
    ;;
  Linux:x86_64)
    MITASE_RELEASE_TARGET="x86_64-unknown-linux-gnu"
    MITASE_ARCHIVE_SHA256="f56a35b350b8ac53c0c747fbaa978b7dafc30da426cb1ab618bf2984d17052ab"
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
