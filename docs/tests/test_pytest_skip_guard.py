"""REQ-OPS-033 coverage for the skipped-test JUnit guard."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast

if TYPE_CHECKING:
    import pytest

SKIPPED_COUNT = 2


class _SkipGuardModule(Protocol):
    def count_skipped_tests(self, report_path: Path) -> int:
        pass

    def main(self, argv: list[str]) -> int:
        pass


def _load_skip_guard_module() -> _SkipGuardModule:
    module_path = (
        Path(__file__).resolve().parents[2] / "scripts" / "fail_on_skipped_tests.py"
    )
    spec = importlib.util.spec_from_file_location("fail_on_skipped_tests", module_path)
    if spec is None or spec.loader is None:
        message = "failed to load scripts/fail_on_skipped_tests.py"
        raise AssertionError(message)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return cast("_SkipGuardModule", module)


_SKIP_GUARD = _load_skip_guard_module()
count_skipped_tests = _SKIP_GUARD.count_skipped_tests
main = _SKIP_GUARD.main


def _write_report(path: Path, *, skipped: int) -> None:
    path.write_text(
        (
            '<?xml version="1.0" encoding="utf-8"?>'
            f'<testsuite tests="1" failures="0" skipped="{skipped}"></testsuite>'
        ),
        encoding="utf-8",
    )


def test_req_ops_033_skip_guard_accepts_clean_report(tmp_path: Path) -> None:
    """REQ-OPS-033: skipped-test guard accepts a report with zero skips."""
    report_path = tmp_path / "clean.xml"
    _write_report(report_path, skipped=0)

    skipped = count_skipped_tests(report_path)
    exit_code = main(["fail_on_skipped_tests.py", str(report_path), "backend tests"])
    if skipped != 0:
        message = f"expected zero skipped tests, got {skipped}"
        raise AssertionError(message)
    if exit_code != 0:
        message = f"expected zero exit code, got {exit_code}"
        raise AssertionError(message)


def test_req_ops_033_skip_guard_rejects_skipped_report(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """REQ-OPS-033: skipped-test guard rejects a report with skipped tests."""
    report_path = tmp_path / "skipped.xml"
    _write_report(report_path, skipped=SKIPPED_COUNT)

    skipped = count_skipped_tests(report_path)
    exit_code = main(["fail_on_skipped_tests.py", str(report_path), "backend tests"])
    stderr = capsys.readouterr().err
    if skipped != SKIPPED_COUNT:
        message = f"expected {SKIPPED_COUNT} skipped tests, got {skipped}"
        raise AssertionError(message)
    if exit_code != 1:
        message = f"expected exit code 1, got {exit_code}"
        raise AssertionError(message)
    if f"backend tests: skipped={SKIPPED_COUNT} is not allowed" not in stderr:
        message = "skip guard must report the backend skipped-test error"
        raise AssertionError(message)
