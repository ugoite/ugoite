"""Enforce the ugoite-minimum corrected 100% line-coverage gate."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
MINIMUM_ROOT = PROJECT_ROOT / "ugoite-minimum"
SOURCE_ROOT = MINIMUM_ROOT / "src"
DELIMITER_ONLY_LINE = re.compile(r"^[\s\]\[\(\)\{\},;]+$")
ENV_DEFAULTS = {
    "CARGO_INCREMENTAL": "0",
    "CARGO_PROFILE_DEV_DEBUG": "0",
    "CARGO_PROFILE_TEST_DEBUG": "0",
    "CARGO_BUILD_JOBS": "1",
    "RUSTFLAGS": "-C debuginfo=0",
}


@dataclass(frozen=True)
class _FileCoverage:
    path: Path
    raw_percent: float
    raw_total: int
    raw_covered: int
    corrected_total: int
    corrected_covered: int
    uncovered_lines: tuple[str, ...]

    @property
    def corrected_percent(self) -> float:
        return self.effective_covered / self.effective_total * 100.0

    @property
    def effective_total(self) -> int:
        return self.corrected_total or self.raw_total

    @property
    def effective_covered(self) -> int:
        return self.corrected_covered if self.corrected_total else self.raw_covered


def _run(command: list[str], *, env: dict[str, str]) -> None:
    subprocess.run(command, cwd=MINIMUM_ROOT, env=env, check=True)


def _is_substantive_line(text: str) -> bool:
    stripped = text.strip()
    return bool(stripped) and DELIMITER_ONLY_LINE.fullmatch(stripped) is None


def _load_coverage(json_path: Path) -> list[_FileCoverage]:
    data = json.loads(json_path.read_text(encoding="utf-8"))
    files: list[_FileCoverage] = []

    for file_entry in data["data"][0]["files"]:
        file_path = Path(file_entry["filename"])
        try:
            rel_path = file_path.relative_to(PROJECT_ROOT)
        except ValueError:
            continue
        if SOURCE_ROOT not in file_path.parents:
            continue

        source_lines = file_path.read_text(encoding="utf-8").splitlines()
        executable_lines: dict[int, bool] = {}
        for line, _col, count, has_count, _is_region_entry, is_gap in file_entry.get(
            "segments",
            [],
        ):
            if not has_count or is_gap or not (1 <= line <= len(source_lines)):
                continue
            if not _is_substantive_line(source_lines[line - 1]):
                continue
            executable_lines[line] = executable_lines.get(line, False) or count > 0

        uncovered_lines = tuple(
            f"{rel_path}:{line}: {source_lines[line - 1].strip()}"
            for line, covered in sorted(executable_lines.items())
            if not covered
        )
        raw_percent = float(file_entry["summary"]["lines"]["percent"])
        raw_total = int(file_entry["summary"]["lines"]["count"])
        raw_covered = int(file_entry["summary"]["lines"]["covered"])
        corrected_total = len(executable_lines)
        corrected_covered = corrected_total - len(uncovered_lines)
        files.append(
            _FileCoverage(
                path=rel_path,
                raw_percent=raw_percent,
                raw_total=raw_total,
                raw_covered=raw_covered,
                corrected_total=corrected_total,
                corrected_covered=corrected_covered,
                uncovered_lines=uncovered_lines,
            ),
        )

    return files


def _write(message: str) -> None:
    sys.stdout.write(f"{message}\n")


def _write_summary(files: list[_FileCoverage]) -> None:
    _write("ugoite-minimum corrected line coverage")
    for file_cov in files:
        _write(
            f"- {file_cov.path}: raw {file_cov.raw_percent:.2f}% -> "
            f"corrected {file_cov.corrected_percent:.2f}% "
            f"({file_cov.effective_covered}/{file_cov.effective_total})",
        )


def _main() -> int:
    env = os.environ.copy()
    env.update(ENV_DEFAULTS)

    with tempfile.TemporaryDirectory() as tmpdir:
        json_path = Path(tmpdir) / "minimum-coverage.json"
        _run(["cargo", "clean", "-p", "ugoite-minimum"], env=env)
        _run(["cargo", "llvm-cov", "clean", "--workspace"], env=env)
        _run(
            [
                "cargo",
                "llvm-cov",
                "--test",
                "test_coverage",
                "--json",
                "--output-path",
                str(json_path),
                "--jobs",
                "1",
            ],
            env=env,
        )

        files = _load_coverage(json_path)
        _write_summary(files)

        failing_files = [
            file_cov
            for file_cov in files
            if file_cov.effective_covered != file_cov.effective_total
        ]
        if not failing_files:
            return 0

        _write("")
        _write("Uncovered substantive lines:")
        for file_cov in failing_files:
            for uncovered_line in file_cov.uncovered_lines:
                _write(f"- {uncovered_line}")

        subprocess.run(
            ["cargo", "llvm-cov", "report", "--text", "--show-missing-lines"],
            cwd=MINIMUM_ROOT,
            env=env,
            check=False,
        )
        return 1


if __name__ == "__main__":
    sys.exit(_main())
