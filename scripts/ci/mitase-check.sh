#!/usr/bin/env bash
set -euo pipefail

MITASE_REVISION="${MITASE_REVISION:-8a306ead996ae2789422303b213fa97000394285}"
MITASE_REPOSITORY="${MITASE_REPOSITORY:-https://github.com/ugoite/syu.git}"
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
