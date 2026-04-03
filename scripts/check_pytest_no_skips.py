"""Fail a pytest JUnit XML report when any tests were skipped."""

from __future__ import annotations

import argparse
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fail when a pytest JUnit XML report contains skipped tests.",
    )
    parser.add_argument("report", type=Path, help="Path to the pytest JUnit XML report")
    parser.add_argument(
        "label",
        help="Human-readable label to include in success and failure messages",
    )
    return parser.parse_args()


def _collect_suites(root: ET.Element) -> list[ET.Element]:
    return [root] if root.tag == "testsuite" else list(root.findall("testsuite"))


def _count_attr(suites: list[ET.Element], name: str) -> int:
    return sum(int(float(suite.attrib.get(name, "0") or 0)) for suite in suites)


def _main() -> int:
    args = _parse_args()
    root = ET.parse(args.report).getroot()
    suites = _collect_suites(root)
    skipped = _count_attr(suites, "skipped")
    if skipped > 0:
        raise SystemExit(f"{args.label}: skipped={skipped} is not allowed")

    tests = _count_attr(suites, "tests")
    sys.stdout.write(f"{args.label} OK: tests={tests}, skipped={skipped}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
