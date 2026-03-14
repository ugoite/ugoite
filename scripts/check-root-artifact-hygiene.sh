#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

bash scripts/check-placeholder-artifacts.sh

python3 - <<'PY'
from __future__ import annotations

import subprocess

tracked_ignored = [
    path
    for path in subprocess.check_output(
        ["git", "ls-files", "-ci", "--exclude-standard"],
        text=True,
    ).splitlines()
    if path
]

if tracked_ignored:
    lines = [
        "Tracked files must not also match ignore rules:",
        *[f"  - {path}" for path in tracked_ignored],
        "",
        "Either untrack the artifact or narrow the ignore rules so intentional",
        "source files do not appear as tracked+ignored.",
    ]
    raise SystemExit("\n".join(lines))

print("Repository artifact hygiene check passed.")
PY
