#!/usr/bin/env python3

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=True, text=True, capture_output=True)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Create a GitHub pull request from a checked-in body file.",
    )
    parser.add_argument("--title", required=True)
    parser.add_argument("--body-file", required=True)
    parser.add_argument("--base", default="main")
    parser.add_argument("--head")
    parser.add_argument("--draft", action="store_true")
    args = parser.parse_args()

    body_path = pathlib.Path(args.body_file)
    if not body_path.is_file():
        raise SystemExit(f"PR body file not found: {body_path}")

    cmd = [
        "gh",
        "pr",
        "create",
        "--title",
        args.title,
        "--body-file",
        str(body_path),
        "--base",
        args.base,
    ]
    if args.head:
        cmd.extend(["--head", args.head])
    if args.draft:
        cmd.append("--draft")

    completed = run(cmd)
    sys.stdout.write(completed.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
