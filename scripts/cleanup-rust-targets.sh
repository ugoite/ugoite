#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_target="${repo_root}/target/rust"
rm -rf "${shared_target}"
echo "Removed shared Rust target cache: ${shared_target}"
