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
ROUTES_DIR = REPO_ROOT / "frontend" / "src" / "routes"


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


def _route_path_from_file(file_path: Path) -> str:
    try:
        relative = file_path.resolve().relative_to(ROUTES_DIR.resolve())
    except ValueError as exc:
        message = f"Frontend file {file_path} is not under {ROUTES_DIR}"
        raise AssertionError(message) from exc

    parts = list(relative.parts)
    if not parts:
        return "/"

    last = parts[-1]
    if last.endswith((".ts", ".tsx")):
        parts[-1] = Path(last).stem

    mapped: list[str] = []
    for part in parts:
        if part == "index":
            continue
        mapped_part = part
        if mapped_part.startswith("[") and mapped_part.endswith("]"):
            mapped_part = "{" + mapped_part[1:-1] + "}"
        mapped.append(mapped_part)

    if not mapped:
        return "/"
    return "/" + "/".join(mapped)


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
    elif suffix == ".rs":
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


def test_frontend_paths_match_routes() -> None:
    """REQ-API-004: Frontend paths must match UI route files."""
    entries = _iter_api_entries()
    for api in entries:
        api_id = api.get("id", "<unknown>")
        frontend = api.get("frontend")
        if not isinstance(frontend, dict):
            message = f"Missing frontend section for {api_id}"
            raise TypeError(message)

        path_value = str(frontend.get("path") or "")
        file_value = frontend.get("file")
        if not file_value or not path_value:
            message = f"Missing frontend path/file for {api_id}"
            raise AssertionError(message)

        if path_value.startswith("/api"):
            message = f"Frontend path must not include /api for {api_id}"
            raise AssertionError(message)

        file_path = (REPO_ROOT / file_value).resolve()
        expected_path = _route_path_from_file(file_path)
        if expected_path != path_value:
            message = (
                f"Frontend path mismatch for {api_id}: "
                f"expected {expected_path}, got {path_value}"
            )
            raise AssertionError(message)
