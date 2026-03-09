#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_target="${repo_root}/target/rust"
legacy_target=""

if [ -n "${HOME:-}" ]; then
  legacy_target="${HOME}/.cache/ugoite/ugoite-core/target"
fi

rm -rf "${shared_target}"
echo "Removed shared Rust target cache: ${shared_target}"

if [ -n "${legacy_target}" ]; then
  rm -rf "${legacy_target}"
  echo "Removed legacy ugoite-core Rust cache: ${legacy_target}"
fi
