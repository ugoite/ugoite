"""Requirements automation tests.

REQ-API-005: Requirements must be mapped to tests across modules.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

import yaml

if TYPE_CHECKING:
    from collections.abc import Iterable

REPO_ROOT = Path(__file__).resolve().parents[2]
REQUIREMENTS_DIR = REPO_ROOT / "docs" / "spec" / "requirements"
USER_MANAGEMENT_MILESTONE = (
    REPO_ROOT / "docs" / "version" / "v0.1" / "user-management.yaml"
)
USER_MANAGEMENT_COVERAGE_TASK = (
    "Requirement coverage check confirms all new REQ-* are linked to tests"
)

REQ_ID_PATTERN = re.compile(r"REQ-[A-Z]+-\d{3}")


@dataclass(frozen=True)
class RequirementTest:
    """Declared test mapping for a requirement."""

    kind: str
    file: Path
    tests: tuple[str, ...]


@dataclass(frozen=True)
class Requirement:
    """Requirement definition parsed from YAML."""

    source: Path
    req_id: str
    title: str
    description: str
    tests: tuple[RequirementTest, ...]


TEST_SCAN_RULES: tuple[tuple[Path, tuple[str, ...]], ...] = (
    (REPO_ROOT / "backend" / "tests", ("test_*.py",)),
    (REPO_ROOT / "ugoite-minimum" / "tests", ("test_*.rs",)),
    (REPO_ROOT / "ugoite-cli" / "tests", ("test_*.rs",)),
    (REPO_ROOT / "ugoite-core" / "tests", ("test_*.rs",)),
    (REPO_ROOT / "frontend" / "src", ("*.test.ts", "*.test.tsx")),
)


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _iter_requirement_files() -> Iterable[Path]:
    return sorted(REQUIREMENTS_DIR.glob("*.yaml"))


def _requirement_tests(
    req: dict[str, Any],
    source: Path,
) -> tuple[RequirementTest, ...]:
    tests_section = req.get("tests")
    req_id = req.get("id", "<missing>")
    if not isinstance(tests_section, dict):
        message = f"Requirement {req_id} in {source} must define tests"
        raise TypeError(message)

    collected: list[RequirementTest] = []
    for kind, entries in tests_section.items():
        if not isinstance(entries, list):
            message = f"Requirement {req_id} tests.{kind} must be a list"
            raise TypeError(message)
        for entry in entries:
            if not isinstance(entry, dict):
                message = f"Requirement {req_id} tests.{kind} entries must be mappings"
                raise TypeError(message)
            file_value = entry.get("file")
            test_list = entry.get("tests")
            if not file_value or not isinstance(test_list, list) or not test_list:
                message = (
                    f"Requirement {req_id} tests.{kind} must specify file and tests"
                )
                raise AssertionError(message)
            tests = tuple(str(name) for name in test_list)
            collected.append(
                RequirementTest(kind=kind, file=REPO_ROOT / file_value, tests=tests),
            )

    if not collected:
        message = f"Requirement {req_id} must list at least one test"
        raise AssertionError(message)

    return tuple(collected)


def _load_requirements() -> tuple[Requirement, ...]:
    requirements: list[Requirement] = []
    for path in _iter_requirement_files():
        data = _load_yaml(path)
        req_entries = data.get("requirements")
        if not isinstance(req_entries, list):
            message = f"Expected requirements list in {path}"
            raise TypeError(message)
        for req in req_entries:
            if not isinstance(req, dict):
                message = f"Requirement entries in {path} must be mappings"
                raise TypeError(message)
            req_id = str(req.get("id") or "").strip()
            title = str(req.get("title") or "").strip()
            description = str(req.get("description") or "").strip()
            if not req_id or not title or not description:
                message = f"Requirement in {path} missing id/title/description"
                raise AssertionError(message)
            tests = _requirement_tests(req, path)
            requirements.append(
                Requirement(
                    source=path,
                    req_id=req_id,
                    title=title,
                    description=description,
                    tests=tests,
                ),
            )
    return tuple(requirements)


def _load_user_management_milestone() -> dict[str, Any]:
    return _load_yaml(USER_MANAGEMENT_MILESTONE)


def _load_user_management_requirement_ids() -> tuple[str, ...]:
    milestone = _load_user_management_milestone()
    req_ids = milestone.get("requirement_ids")
    if not isinstance(req_ids, list) or not req_ids:
        message = (
            "docs/version/v0.1/user-management.yaml must define non-empty "
            "requirement_ids"
        )
        raise AssertionError(message)

    normalized: tuple[str, ...] = tuple(str(req_id).strip() for req_id in req_ids)
    if any(not req_id for req_id in normalized):
        message = (
            "docs/version/v0.1/user-management.yaml requirement_ids must contain "
            "non-empty REQ-* IDs"
        )
        raise AssertionError(message)
    invalid = [req_id for req_id in normalized if not REQ_ID_PATTERN.fullmatch(req_id)]
    if invalid:
        message = (
            "docs/version/v0.1/user-management.yaml requirement_ids contains "
            f"invalid REQ-* IDs: {', '.join(sorted(invalid))}"
        )
        raise AssertionError(message)
    return normalized


def _is_user_management_coverage_task_done() -> bool:
    milestone = _load_user_management_milestone()
    phases = milestone.get("phases")
    if not isinstance(phases, list):
        message = "docs/version/v0.1/user-management.yaml must define phases"
        raise TypeError(message)

    for phase in phases:
        if not isinstance(phase, dict) or phase.get("id") != "testing":
            continue
        tasks = phase.get("tasks")
        if not isinstance(tasks, list):
            message = (
                "docs/version/v0.1/user-management.yaml testing phase must define tasks"
            )
            raise TypeError(message)
        for task in tasks:
            if not isinstance(task, dict):
                continue
            if task.get("description") == USER_MANAGEMENT_COVERAGE_TASK:
                done = task.get("done")
                if not isinstance(done, bool):
                    message = (
                        "docs/version/v0.1/user-management.yaml user-management "
                        "coverage task must define boolean done flag"
                    )
                    raise AssertionError(message)
                return done
        message = (
            "docs/version/v0.1/user-management.yaml missing user-management "
            "requirement coverage task"
        )
        raise AssertionError(message)

    message = "docs/version/v0.1/user-management.yaml missing testing phase"
    raise AssertionError(message)


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _find_req_ids(text: str) -> set[str]:
    return set(REQ_ID_PATTERN.findall(text))


def _file_has_tests(file_path: Path) -> bool:
    contents = _read_text(file_path)
    suffix = file_path.suffix
    if suffix == ".py":
        return bool(re.search(r"^\s*def\s+test_", contents, re.MULTILINE))
    if suffix == ".rs":
        return "#[test]" in contents
    if suffix in {".ts", ".tsx"}:
        return bool(re.search(r"\b(it|test)\s*\(", contents))
    return False


def _iter_test_files() -> Iterable[Path]:
    for root, patterns in TEST_SCAN_RULES:
        if not root.exists():
            continue
        for pattern in patterns:
            for file_path in root.rglob(pattern):
                if _file_has_tests(file_path):
                    yield file_path


def _assert_tests_declared_exist(requirements: Iterable[Requirement]) -> None:
    for requirement in requirements:
        for test_entry in requirement.tests:
            if not test_entry.file.exists():
                message = (
                    f"Requirement {requirement.req_id} references missing file "
                    f"{test_entry.file.relative_to(REPO_ROOT)}"
                )
                raise AssertionError(message)
            contents = _read_text(test_entry.file)
            for test_name in test_entry.tests:
                if test_name not in contents:
                    message = (
                        f"Requirement {requirement.req_id} references missing test "
                        f"'{test_name}' in {test_entry.file.relative_to(REPO_ROOT)}"
                    )
                    raise AssertionError(message)


def test_requirement_ids_are_unique() -> None:
    """REQ-API-005: Requirement IDs must be unique."""
    requirements = _load_requirements()
    seen: set[str] = set()
    duplicates: set[str] = set()
    for requirement in requirements:
        if requirement.req_id in seen:
            duplicates.add(requirement.req_id)
        seen.add(requirement.req_id)
    if duplicates:
        message = f"Duplicate requirement IDs found: {', '.join(sorted(duplicates))}"
        raise AssertionError(message)


def test_required_fields_present() -> None:
    """REQ-API-005: Required fields must be present in requirements."""
    _load_requirements()


def test_all_requirements_have_tests() -> None:
    """REQ-API-005: Requirements must list tests and files must exist."""
    requirements = _load_requirements()
    _assert_tests_declared_exist(requirements)


def test_all_tests_reference_valid_requirements() -> None:
    """REQ-API-005: All REQ-* references in tests must be valid."""
    requirements = _load_requirements()
    valid_ids = {req.req_id for req in requirements}
    invalid_refs: dict[Path, set[str]] = {}
    for test_file in _iter_test_files():
        contents = _read_text(test_file)
        found = _find_req_ids(contents)
        invalid = {req_id for req_id in found if req_id not in valid_ids}
        if invalid:
            invalid_refs[test_file] = invalid

    if invalid_refs:
        details = ", ".join(
            f"{path.relative_to(REPO_ROOT)}: {sorted(ids)}"
            for path, ids in sorted(invalid_refs.items())
        )
        message = f"Invalid requirement references found: {details}"
        raise AssertionError(message)


def test_no_orphan_tests() -> None:
    """REQ-API-005: Fail when test files lack requirement references."""
    missing: list[str] = []
    for test_file in _iter_test_files():
        contents = _read_text(test_file)
        if not _find_req_ids(contents):
            missing.append(str(test_file.relative_to(REPO_ROOT)))
    if missing:
        message = "Test files missing requirement references: " + ", ".join(missing)
        raise AssertionError(message)


def test_user_management_requirements_have_tests() -> None:
    """REQ-API-005: User-management requirements must map to declared tests."""
    requirements = {
        requirement.req_id: requirement for requirement in _load_requirements()
    }
    req_ids = _load_user_management_requirement_ids()
    missing = sorted(req_id for req_id in req_ids if req_id not in requirements)
    if missing:
        message = (
            "User-management requirement_ids are missing from docs/spec/requirements: "
            + ", ".join(missing)
        )
        raise AssertionError(message)
    _assert_tests_declared_exist(requirements[req_id] for req_id in req_ids)


def test_user_management_requirement_coverage_task_completed() -> None:
    """REQ-API-005: User-management coverage task must be marked complete."""
    if not _is_user_management_coverage_task_done():
        message = (
            "docs/version/v0.1/user-management.yaml must set done: true for "
            "the user-management requirement coverage task after validation"
        )
        raise AssertionError(message)
