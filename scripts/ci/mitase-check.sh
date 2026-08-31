#!/usr/bin/env bash
set -euo pipefail

MITASE_REVISION="${MITASE_REVISION:-5df29996bb3a2e05e0431eb929170f6ef33d1e13}"
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
