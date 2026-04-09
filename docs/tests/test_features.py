"""Feature registry consistency tests.

REQ-API-004: Feature registry paths and functions must exist.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
FEATURES_DIR = REPO_ROOT / "docs" / "spec" / "features"
POLICIES_README = REPO_ROOT / "docs" / "spec" / "policies" / "README.md"
ENDPOINTS_DIR = REPO_ROOT / "backend" / "src" / "app" / "api" / "endpoints"
ROUTES_DIR = REPO_ROOT / "frontend" / "src" / "routes"
DOCSITE_SPEC_DATA = REPO_ROOT / "docsite" / "src" / "lib" / "spec-data.ts"
DOCSITE_FEATURES_PAGE = (
    REPO_ROOT / "docsite" / "src" / "pages" / "design" / "features.astro"
)
DOCSITE_RELATIONS_PAGE = (
    REPO_ROOT / "docsite" / "src" / "pages" / "design" / "relations.astro"
)


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _extract_heading_section(
    contents: str,
    *,
    heading: str,
    next_heading: str | None,
) -> str:
    start = contents.find(heading)
    if start < 0:
        message = f"Missing heading {heading!r}"
        raise AssertionError(message)
    start += len(heading)
    if next_heading is None:
        return contents[start:]
    end = contents.find(next_heading, start)
    if end < 0:
        message = f"Missing heading {next_heading!r}"
        raise AssertionError(message)
    return contents[start:end]


def _load_feature_manifest_files() -> tuple[str, ...]:
    manifest = _load_yaml(FEATURES_DIR / "features.yaml")
    entries = manifest.get("files", [])
    if not isinstance(entries, list):
        message = "Expected list of files in docs/spec/features/features.yaml"
        raise TypeError(message)
    files: list[str] = []
    for entry in entries:
        if not isinstance(entry, dict):
            message = "Feature manifest entries must be mappings"
            raise TypeError(message)
        file_name = str(entry.get("file") or "").strip()
        if not file_name:
            message = "Feature manifest entries must declare a file"
            raise AssertionError(message)
        files.append(file_name)
    return tuple(files)


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


def _assert_feature_section(
    api_id: str,
    section_key: str,
    section: dict[str, Any],
) -> None:
    file_value = section.get("file")
    function_value = section.get("function")
    if not file_value or not function_value:
        message = f"Missing {section_key} file/function for {api_id}"
        raise AssertionError(message)

    if str(file_value).strip().lower() in {"n/a", "na"}:
        message = "Feature registry must not use n/a for file paths"
        raise AssertionError(message)

    file_path = REPO_ROOT / str(file_value)
    if not file_path.exists():
        message = f"Missing file {file_value}"
        raise AssertionError(message)
    _assert_function_exists(file_path, str(function_value))


def _assert_cli_section(api_id: str, cli: dict[str, Any]) -> None:
    if not cli.get("command"):
        message = f"Missing ugoite_cli command for {api_id}"
        raise AssertionError(message)
    _assert_feature_section(api_id, "ugoite_cli", cli)


def _contains_route_handlers(file_path: Path) -> bool:
    contents = _read_text(file_path)
    return "@router." in contents


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
        for section_key in ("backend", "frontend", "ugoite_core"):
            section = api.get(section_key)
            if not isinstance(section, dict):
                message = f"Missing {section_key} section for {api_id}"
                raise TypeError(message)
            _assert_feature_section(api_id, section_key, section)

        cli = api.get("ugoite_cli")
        if cli is None:
            continue
        if not isinstance(cli, dict):
            message = f"Invalid ugoite_cli section for {api_id}"
            raise TypeError(message)
        _assert_cli_section(api_id, cli)


def test_no_undeclared_feature_modules() -> None:
    """REQ-API-004: API endpoint modules must be declared in features specs."""
    entries = _iter_api_entries()
    declared_backend_files = {
        (REPO_ROOT / entry["backend"]["file"]).resolve()
        for entry in entries
        if isinstance(entry.get("backend"), dict) and entry["backend"].get("file")
    }
    actual_backend_files = {
        path.resolve()
        for path in ENDPOINTS_DIR.glob("*.py")
        if path.name != "__init__.py" and _contains_route_handlers(path)
    }

    extras = sorted(actual_backend_files - declared_backend_files)
    if extras:
        extras_list = ", ".join(str(path.relative_to(REPO_ROOT)) for path in extras)
        message = f"Undeclared endpoint modules: {extras_list}"
        raise AssertionError(message)


def test_docs_req_api_004_feature_readme_inventory_matches_manifest() -> None:
    """REQ-API-004: Feature README inventory must mirror the manifest."""
    readme = _read_text(FEATURES_DIR / "README.md")
    inventory_section = _extract_heading_section(
        readme,
        heading="## Inventory",
        next_heading="## Supplemental References",
    )
    expected_files = ("features.yaml", *_load_feature_manifest_files())
    missing = [
        file_name for file_name in expected_files if file_name not in inventory_section
    ]
    unexpected = [
        file_name
        for file_name in ("links.yaml", "sql.md")
        if file_name in inventory_section
    ]
    if missing or unexpected:
        details: list[str] = []
        if missing:
            details.append("missing inventory entries: " + ", ".join(missing))
        if unexpected:
            details.append(
                "supplemental references listed as inventory: " + ", ".join(unexpected),
            )
        message = (
            "docs/spec/features/README.md inventory is out of sync with "
            "docs/spec/features/features.yaml: " + "; ".join(details)
        )
        raise AssertionError(message)


def test_docs_req_api_004_feature_readme_links_browser_modes_to_specs() -> None:
    """REQ-API-004: Feature README must explain where browser authoring modes live."""
    readme = _read_text(FEATURES_DIR / "README.md")
    required_fragments = (
        "../architecture/frontend-backend-interface.md",
        "../requirements/frontend.yaml",
        "Markdown, Web form, and Chat Q&A",
        "REQ-FE-037",
        "REQ-FE-057",
        "API-operation oriented",
    )
    missing = [fragment for fragment in required_fragments if fragment not in readme]
    if missing:
        message = (
            "docs/spec/features/README.md must point browser authoring modes to "
            "their canonical frontend specs: " + ", ".join(missing)
        )
        raise AssertionError(message)


def test_docs_req_api_004_docsite_feature_pages_use_manifest_loader() -> None:
    """REQ-API-004: Docsite feature pages must use the manifest-backed loader."""
    spec_data = _read_text(DOCSITE_SPEC_DATA)
    if 'path.join(specRoot, "features/features.yaml")' not in spec_data:
        message = (
            "docsite/src/lib/spec-data.ts must load docs/spec/features/features.yaml"
        )
        raise AssertionError(message)

    missing_calls: list[str] = []
    expected_calls = {
        DOCSITE_FEATURES_PAGE: "await getFeatureGroups()",
        DOCSITE_RELATIONS_PAGE: "await getFeatureGroups()",
    }
    for path, marker in expected_calls.items():
        contents = _read_text(path)
        if "getFeatureGroups" not in contents or marker not in contents:
            missing_calls.append(str(path.relative_to(REPO_ROOT)))
    if missing_calls:
        message = (
            "Docsite feature pages must read feature groups via getFeatureGroups(): "
            + ", ".join(missing_calls)
        )
        raise AssertionError(message)


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


def test_docs_req_api_004_policy_traceability_docs_describe_manifest_links() -> None:
    """REQ-API-004: Policy docs must describe manifest-backed feature links."""
    readme = _read_text(POLICIES_README)
    required_snippets = (
        "linked_requirements",
        "docs/spec/features/features.yaml",
        "Feature-area badges",
    )
    missing = [snippet for snippet in required_snippets if snippet not in readme]
    if missing:
        message = (
            "docs/spec/policies/README.md must explain manifest-backed"
            " feature traceability: " + ", ".join(missing)
        )
        raise AssertionError(message)
