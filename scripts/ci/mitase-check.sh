#!/usr/bin/env bash
set -euo pipefail

MITASE_REVISION="${MITASE_REVISION:-d882e750511d34e0a17bde01b858cd5631c55399}"
MITASE_REPOSITORY="${MITASE_REPOSITORY:-https://github.com/ugoite/mitase.git}"
MITASE_ROOT="${MITASE_ROOT:-target/mitase-${MITASE_REVISION}}"

if [[ -n "${MITASE_BIN:-}" ]]; then
  exec "$MITASE_BIN" check .
fi

if [[ ! -x "${MITASE_ROOT}/bin/mitase" ]]; then
  cargo install \
    --locked \
    --git "$MITASE_REPOSITORY" \
    --rev "$MITASE_REVISION" \
    --root "$MITASE_ROOT" \
    mitase
fi

exec "${MITASE_ROOT}/bin/mitase" check .
