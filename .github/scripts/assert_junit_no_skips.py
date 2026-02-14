from __future__ import annotations

import argparse
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


def _to_int(value: str | None) -> int:
    if value is None or value == "":
        return 0
    try:
        return int(float(value))
    except ValueError:
        return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Fail CI if JUnit report contains skipped tests")
    parser.add_argument("report", type=Path, help="Path to JUnit XML file")
    parser.add_argument("--label", default="tests", help="Label for error messages")
    args = parser.parse_args()

    if not args.report.exists():
        print(f"ERROR: JUnit report not found: {args.report}", file=sys.stderr)
        return 2

    root = ET.parse(args.report).getroot()
    suites = [root] if root.tag == "testsuite" else list(root.findall("testsuite"))
    if not suites:
        print(f"ERROR: No testsuite nodes found in {args.report}", file=sys.stderr)
        return 2

    skipped = sum(_to_int(suite.attrib.get("skipped")) for suite in suites)
    tests = sum(_to_int(suite.attrib.get("tests")) for suite in suites)
    failures = sum(_to_int(suite.attrib.get("failures")) for suite in suites)
    errors = sum(_to_int(suite.attrib.get("errors")) for suite in suites)

    if tests == 0:
        print(f"ERROR: {args.label} report has zero executed tests ({args.report})", file=sys.stderr)
        return 1

    if skipped > 0:
        print(
            f"ERROR: {args.label} report contains skipped tests: skipped={skipped}, tests={tests}, failures={failures}, errors={errors}",
            file=sys.stderr,
        )
        return 1

    print(
        f"OK: {args.label} report has no skipped tests: tests={tests}, failures={failures}, errors={errors}",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())