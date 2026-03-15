#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

bash scripts/check-placeholder-artifacts.sh

python3 - <<'PY'
from __future__ import annotations

import os
import subprocess
from pathlib import PurePosixPath

MAX_TRACKED_FILE_BYTES = 1024 * 1024  # 1 MiB
FORBIDDEN_SEGMENTS = {"node_modules", "target"}
ALLOWED_LARGE_TRACKED_PATHS: set[str] = set()


def format_bytes(size: int) -> str:
    """Return a human-readable byte count."""
    units = ["bytes", "KiB", "MiB", "GiB"]
    value = float(size)
    unit = units[0]
    for candidate in units:
        unit = candidate
        if value < 1024 or candidate == units[-1]:
            break
        value /= 1024
    if unit == "bytes":
        return f"{int(value)} {unit}"
    return f"{value:.1f} {unit}"


tracked_paths = subprocess.check_output(
    ["git", "ls-files", "-z"],
    text=False,
).decode("utf-8").split("\0")
tracked_ignored = [
    path
    for path in subprocess.check_output(
        ["git", "ls-files", "-ci", "--exclude-standard"],
        text=True,
    ).splitlines()
    if path
]

tracked_ignored: list[str] = []
forbidden_paths: list[tuple[str, str]] = []
oversized_paths: list[tuple[str, int]] = []

for raw_path in tracked_paths:
    if not raw_path:
        continue

    if subprocess.run(
        ["git", "check-ignore", "--no-index", raw_path],
        capture_output=True,
        check=False,
    ).returncode == 0:
        tracked_ignored.append(raw_path)

    path = PurePosixPath(raw_path)
    forbidden_segment = next(
        (segment for segment in path.parts if segment in FORBIDDEN_SEGMENTS),
        None,
    )
    if forbidden_segment is not None:
        forbidden_paths.append((raw_path, forbidden_segment))
        continue

    try:
        size = os.path.getsize(raw_path)
    except FileNotFoundError:
        continue

    if size > MAX_TRACKED_FILE_BYTES and raw_path not in ALLOWED_LARGE_TRACKED_PATHS:
        oversized_paths.append((raw_path, size))

if tracked_ignored or forbidden_paths or oversized_paths:
    messages: list[str] = []
    if tracked_ignored:
        lines = [
            "Tracked files must not also match ignore rules:",
        ]
        lines.extend(f"  - {path}" for path in sorted(tracked_ignored))
        lines.extend(
            [
                "",
                "Either untrack the artifact or narrow the ignore rules so",
                "intentional source files do not appear as tracked+ignored.",
            ]
        )
        messages.append("\n".join(lines))
    if forbidden_paths:
        lines = [
            "Tracked files must not live under generated dependency/build directories:",
        ]
        lines.extend(
            f"  - {path} (forbidden segment: {segment})"
            for path, segment in sorted(forbidden_paths)
        )
        messages.append("\n".join(lines))
    if oversized_paths:
        lines = [
            "Tracked files larger than 1 MiB must be explicitly allowlisted in "
            "scripts/check-root-artifact-hygiene.sh:",
        ]
        lines.extend(
            f"  - {path} ({format_bytes(size)})"
            for path, size in sorted(oversized_paths)
        )
        messages.append("\n".join(lines))
    raise SystemExit("\n\n".join(messages))

print("Repository artifact hygiene check passed.")
PY
