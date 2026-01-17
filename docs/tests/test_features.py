"""Feature registry consistency tests.

REQ-API-004: Feature registry paths and functions must exist.
"""

from __future__ import annotations

import re
import warnings
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
FEATURES_DIR = REPO_ROOT / "docs" / "spec" / "features"
ENDPOINTS_DIR = REPO_ROOT / "backend" / "src" / "app" / "api" / "endpoints"


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _function_exists_python(contents: str, name: str) -> bool:
    return bool(
        re.search(
            rf"^\s*(async\s+def|def)\s+{re.escape(name)}\b",
            contents,
            re.MULTILINE,
        ),
    )


def _function_exists_rust(contents: str, name: str) -> bool:
    return bool(re.search(rf"\bfn\s+{re.escape(name)}\b", contents))


def _function_exists_typescript(contents: str, name: str) -> bool:
    if "." in name:
        obj, member = name.split(".", 1)
        obj_match = re.search(rf"\b{re.escape(obj)}\b", contents)
        member_match = re.search(rf"\b{re.escape(member)}\s*[:(]", contents)
        return bool(obj_match and member_match)
    return bool(re.search(rf"\b{re.escape(name)}\b", contents))


def _normalize_path(path: str) -> str:
    cleaned = path.strip()
    if cleaned != "/" and cleaned.endswith("/"):
        return cleaned.rstrip("/")
    return cleaned


def _find_function_line_index(lines: list[str], function_name: str) -> int | None:
    pattern = re.compile(rf"^\s*(async\s+def|def)\s+{re.escape(function_name)}\b")
    for index, line in enumerate(lines):
        if pattern.search(line):
            return index
    return None


def _find_route_decorator_start(
    lines: list[str],
    function_index: int,
    method_lower: str,
) -> int | None:
    cursor = function_index - 1
    while cursor >= 0 and not lines[cursor].strip():
        cursor -= 1
    decorator_pattern = re.compile(rf"^\s*@router\.{re.escape(method_lower)}\b")
    def_pattern = re.compile(r"^\s*(async\s+def|def)\b")
    while cursor >= 0:
        if decorator_pattern.match(lines[cursor]):
            return cursor
        if def_pattern.match(lines[cursor]):
            return None
        cursor -= 1
    return None


def _collect_decorator_text(
    lines: list[str],
    decorator_start: int,
    function_index: int,
) -> str:
    decorator_lines: list[str] = []
    paren_depth = 0
    for cursor in range(decorator_start, function_index):
        decorator_lines.append(lines[cursor])
        paren_depth += lines[cursor].count("(") - lines[cursor].count(")")
        if paren_depth <= 0 and cursor != decorator_start:
            break
    return "\n".join(decorator_lines)


def _extract_backend_route_path(
    contents: str,
    function_name: str,
    method: str,
) -> str:
    method_lower = method.lower()
    lines = contents.splitlines()
    function_index = _find_function_line_index(lines, function_name)
    if function_index is None:
        message = f"Missing function definition for {function_name}"
        raise AssertionError(message)
    decorator_start = _find_route_decorator_start(
        lines,
        function_index,
        method_lower,
    )
    if decorator_start is None:
        message = (
            f"Missing FastAPI route decorator for {function_name} ({method.upper()})"
        )
        raise AssertionError(message)
    decorator_text = _collect_decorator_text(lines, decorator_start, function_index)
    path_match = re.search(r"['\"]([^'\"]+)['\"]", decorator_text)
    if not path_match:
        message = f"Missing route path string for {function_name}"
        raise AssertionError(message)
    return path_match.group(1)


def _build_frontend_path_regex(path: str) -> re.Pattern[str]:
    parts = re.split(r"\{[^}]+\}", path)
    pattern = ""
    for index, part in enumerate(parts):
        pattern += re.escape(part)
        if index < len(parts) - 1:
            pattern += r"\$\{[^}]+\}"
    pattern = rf"{pattern}(?:\?[^`'\"]*)?"
    return re.compile(rf"[`'\"]{pattern}[`'\"]")


def _assert_function_exists(file_path: Path, function_name: str) -> None:
    if function_name.lower() in {"n/a", "na"}:
        message = "Feature registry must not use n/a for function names"
        raise AssertionError(message)
    contents = _read_text(file_path)
    suffix = file_path.suffix.lower()
    if suffix == ".py":
        if not _function_exists_python(contents, function_name):
            message = f"Missing Python function {function_name} in {file_path}"
            raise AssertionError(message)
    elif suffix in {".ts", ".tsx"}:
        if not _function_exists_typescript(contents, function_name):
            message = f"Missing TypeScript symbol {function_name} in {file_path}"
            raise AssertionError(message)
    elif suffix in {".rs"}:
        if not _function_exists_rust(contents, function_name):
            message = f"Missing Rust function {function_name} in {file_path}"
            raise AssertionError(message)
    else:
        message = f"Unsupported file type for {file_path}"
        raise AssertionError(message)


def _iter_api_entries() -> list[dict[str, Any]]:
    manifest = _load_yaml(FEATURES_DIR / "features.yaml")
    files = manifest.get("files", [])
    entries: list[dict[str, Any]] = []
    for entry in files:
        feature_file = entry.get("file")
        if not feature_file:
            continue
        registry = _load_yaml(FEATURES_DIR / feature_file)
        apis = registry.get("apis", [])
        if isinstance(apis, list):
            entries.extend([api for api in apis if isinstance(api, dict)])
    return entries


def test_feature_paths_exist() -> None:
    """REQ-API-004: Feature registry entries must exist in code."""
    entries = _iter_api_entries()
    if not entries:
        message = "No API entries found in feature registry"
        raise AssertionError(message)

    for api in entries:
        api_id = api.get("id", "<unknown>")
        for section_key in ("backend", "frontend", "ieapp_core", "ieapp_cli"):
            section = api.get(section_key)
            if not isinstance(section, dict):
                message = f"Missing {section_key} section for {api_id}"
                raise TypeError(message)
            file_value = section.get("file")
            function_value = section.get("function")
            if not file_value or not function_value:
                message = f"Missing {section_key} file/function for {api_id}"
                raise AssertionError(message)

            if str(file_value).strip().lower() in {"n/a", "na"}:
                message = "Feature registry must not use n/a for file paths"
                raise AssertionError(message)

            if section_key == "ieapp_cli" and not section.get("command"):
                message = f"Missing ieapp_cli command for {api_id}"
                raise AssertionError(message)

            file_path = REPO_ROOT / file_value
            if not file_path.exists():
                message = f"Missing file {file_value}"
                raise AssertionError(message)
            _assert_function_exists(file_path, str(function_value))


def test_feature_backend_paths_match_registry() -> None:
    """REQ-API-004: Backend paths must match FastAPI routes."""
    entries = _iter_api_entries()
    for api in entries:
        api_id = api.get("id", "<unknown>")
        method = api.get("method", "GET")
        backend = api.get("backend")
        if not isinstance(backend, dict):
            message = f"Missing backend section for {api_id}"
            raise TypeError(message)
        path_value = backend.get("path")
        file_value = backend.get("file")
        function_value = backend.get("function")
        if not path_value or not file_value or not function_value:
            message = f"Missing backend path/file/function for {api_id}"
            raise AssertionError(message)
        file_path = REPO_ROOT / str(file_value)
        contents = _read_text(file_path)
        actual_path = _extract_backend_route_path(
            contents,
            str(function_value),
            str(method),
        )
        if _normalize_path(actual_path) != _normalize_path(str(path_value)):
            message = (
                f"Backend path mismatch for {api_id}: "
                f"expected {path_value}, got {actual_path}"
            )
            raise AssertionError(message)


def test_feature_frontend_paths_match_registry() -> None:
    """REQ-API-004: Frontend paths must match API client usage."""
    entries = _iter_api_entries()
    for api in entries:
        api_id = api.get("id", "<unknown>")
        frontend = api.get("frontend")
        if not isinstance(frontend, dict):
            message = f"Missing frontend section for {api_id}"
            raise TypeError(message)
        path_value = frontend.get("path")
        file_value = frontend.get("file")
        if not path_value or not file_value:
            message = f"Missing frontend path/file for {api_id}"
            raise AssertionError(message)
        file_path = REPO_ROOT / str(file_value)
        contents = _read_text(file_path)
        path_regex = _build_frontend_path_regex(str(path_value))
        if not path_regex.search(contents):
            message = f"Frontend path {path_value} not found for {api_id}"
            raise AssertionError(message)


def test_no_undeclared_feature_modules() -> None:
    """REQ-API-004: Warn on undeclared API endpoint modules."""
    entries = _iter_api_entries()
    declared_backend_files = {
        (REPO_ROOT / entry["backend"]["file"]).resolve()
        for entry in entries
        if isinstance(entry.get("backend"), dict) and entry["backend"].get("file")
    }
    actual_backend_files = {
        path.resolve()
        for path in ENDPOINTS_DIR.glob("*.py")
        if path.name != "__init__.py"
    }

    extras = sorted(actual_backend_files - declared_backend_files)
    if extras:
        extras_list = ", ".join(str(path.relative_to(REPO_ROOT)) for path in extras)
        warnings.warn(
            f"Undeclared endpoint modules: {extras_list}",
            stacklevel=2,
        )
