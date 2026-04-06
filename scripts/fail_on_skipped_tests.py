"""Fail when a JUnit XML report contains skipped tests."""

from __future__ import annotations

import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ARG_COUNT = 3
USAGE_EXIT_CODE = 2
SKIPPED_TEST_EXIT_CODE = 1


def count_skipped_tests(report_path: Path) -> int:
    """Return the total skipped-test count from a JUnit XML report."""
    root = ET.parse(report_path).getroot()
    suites = [root] if root.tag == "testsuite" else list(root.findall("testsuite"))
    return sum(int(float(suite.attrib.get("skipped", "0") or 0)) for suite in suites)


def main(argv: list[str]) -> int:
    """CLI entrypoint."""
    if len(argv) != ARG_COUNT:
        sys.stderr.write(
            "Usage: python scripts/fail_on_skipped_tests.py <junit-xml> <label>\n",
        )
        return USAGE_EXIT_CODE

    report_path = Path(argv[1])
    label = argv[2]
    skipped = count_skipped_tests(report_path)
    if skipped > 0:
        sys.stderr.write(f"{label}: skipped={skipped} is not allowed\n")
        return SKIPPED_TEST_EXIT_CODE
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
