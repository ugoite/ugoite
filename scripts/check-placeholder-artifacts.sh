#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mapfile -t placeholder_files < <(
  python3 - <<'PY'
from pathlib import Path

SENTINEL = "This file is intentionally left blank."
root = Path(".")

for path in sorted(root.iterdir()):
    if not path.is_file() or path.name.startswith("."):
        continue
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    if text.strip() == SENTINEL:
        print(path.as_posix())
PY
)

if [ "${#placeholder_files[@]}" -gt 0 ]; then
  echo "Found placeholder root artifacts that must be removed:" >&2
  printf "  - %s\n" "${placeholder_files[@]}" >&2
  exit 1
fi

echo "Repository root placeholder artifact check passed."
