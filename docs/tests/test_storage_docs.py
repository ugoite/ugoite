"""Storage documentation consistency tests.

REQ-STO-001: Storage docs must declare OpenDAL as the current runtime layer.
REQ-STO-007: Backend/core storage boundary docs must match the shared runtime.
REQ-STO-011: Storage layout docs must match runtime-created paths and schemas.
"""

from __future__ import annotations

import asyncio
import json
from datetime import datetime
from typing import Any

import ugoite_core
import yaml

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTEND_BACKEND_INTERFACE_PATH = (
    REPO_ROOT / "docs" / "spec" / "architecture" / "frontend-backend-interface.md"
)
ARCHITECTURE_DECISIONS_PATH = (
    REPO_ROOT / "docs" / "spec" / "architecture" / "decisions.md"
)
TESTING_STRATEGY_PATH = REPO_ROOT / "docs" / "spec" / "testing" / "strategy.md"
STORAGE_REQUIREMENTS_PATH = (
    REPO_ROOT / "docs" / "spec" / "requirements" / "storage.yaml"
)
DIRECTORY_LAYOUT_PATH = (
    REPO_ROOT / "docs" / "spec" / "data-model" / "directory-layout.yaml"
)
FILE_SCHEMAS_PATH = REPO_ROOT / "docs" / "spec" / "data-model" / "file-schemas.yaml"


def _load_yaml(path: Path) -> dict[str, Any]:
    loaded = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(loaded, dict):
        raise AssertionError(f"{path.relative_to(REPO_ROOT)} must be a YAML mapping")
    return loaded


def _normalize_whitespace(text: str) -> str:
    return " ".join(text.split())


def _directory_layout() -> dict[str, Any]:
    return _load_yaml(DIRECTORY_LAYOUT_PATH)


def _file_schemas() -> dict[str, Any]:
    schemas = _load_yaml(FILE_SCHEMAS_PATH).get("schemas")
    if not isinstance(schemas, dict):
        raise AssertionError("file-schemas.yaml must define a schemas mapping")
    return schemas


def _render_path(template: str, **values: str) -> Path:
    return Path(template.format(**values))


def _read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _is_type(value: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(value, dict)
    if expected == "array":
        return isinstance(value, list)
    if expected == "string":
        return isinstance(value, str)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "null":
        return value is None
    raise AssertionError(f"Unsupported schema type: {expected}")


def _validate_format(value: Any, expected_format: str, context: str) -> None:
    if value is None:
        return
    if expected_format != "date-time":
        return
    if not isinstance(value, str):
        raise AssertionError(f"{context} must be a date-time string")
    try:
        datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:  # pragma: no cover - exercised on failure
        raise AssertionError(f"{context} must be valid RFC3339: {value}") from exc


def _validate_schema(value: Any, schema: dict[str, Any], context: str) -> None:
    schema_type = schema.get("type")
    if isinstance(schema_type, list):
        if not any(_is_type(value, str(option)) for option in schema_type):
            raise AssertionError(f"{context} must match one of {schema_type!r}")
    elif isinstance(schema_type, str):
        if not _is_type(value, schema_type):
            raise AssertionError(f"{context} must be {schema_type}")

    enum_values = schema.get("enum")
    if isinstance(enum_values, list) and value not in enum_values:
        raise AssertionError(f"{context} must be one of {enum_values!r}")

    expected_format = schema.get("format")
    if isinstance(expected_format, str):
        _validate_format(value, expected_format, context)

    if schema_type == "object":
        if not isinstance(value, dict):
            raise AssertionError(f"{context} must be an object")
        required = schema.get("required", [])
        if isinstance(required, list):
            missing = [str(key) for key in required if key not in value]
            if missing:
                raise AssertionError(f"{context} missing required keys: {missing!r}")
        properties = schema.get("properties", {})
        if isinstance(properties, dict):
            for key, prop_schema in properties.items():
                if key not in value or not isinstance(prop_schema, dict):
                    continue
                _validate_schema(value[key], prop_schema, f"{context}.{key}")
        additional = schema.get("additionalProperties")
        if isinstance(additional, dict):
            handled = set(properties) if isinstance(properties, dict) else set()
            for key, item in value.items():
                if key in handled:
                    continue
                _validate_schema(item, additional, f"{context}.{key}")
        return

    if schema_type == "array":
        if not isinstance(value, list):
            raise AssertionError(f"{context} must be an array")
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, item in enumerate(value):
                _validate_schema(item, item_schema, f"{context}[{index}]")


def test_docs_req_sto_001_storage_runtime_declares_opendal_current() -> None:
    """REQ-STO-001: Storage docs treat OpenDAL as the current runtime layer."""
    interface_text = _normalize_whitespace(
        FRONTEND_BACKEND_INTERFACE_PATH.read_text(encoding="utf-8"),
    )
    decisions_text = _normalize_whitespace(
        ARCHITECTURE_DECISIONS_PATH.read_text(encoding="utf-8"),
    )
    requirement_text = _normalize_whitespace(
        STORAGE_REQUIREMENTS_PATH.read_text(encoding="utf-8"),
    )

    details: list[str] = []
    if "transitioning to OpenDAL" in interface_text:
        details.append(
            "frontend-backend-interface.md must not describe the runtime as "
            "transitioning to OpenDAL",
        )
    if "OpenDAL-backed adapter" not in interface_text:
        details.append(
            "frontend-backend-interface.md must describe the current runtime "
            "adapter as OpenDAL-backed",
        )
    if "historical context only" not in interface_text:
        details.append(
            "frontend-backend-interface.md must describe fsspec as historical "
            "context only",
        )
    if (
        "fsspec" not in decisions_text
        or "no longer part of the active runtime storage architecture"
        not in decisions_text
    ):
        details.append(
            "decisions.md must describe fsspec as no longer part of the active "
            "runtime storage architecture",
        )
    if "current runtime adapter is OpenDAL-backed" not in requirement_text:
        details.append(
            "requirements/storage.yaml must state that the current runtime "
            "adapter is OpenDAL-backed",
        )
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_sto_011_storage_layout_bootstrap_matches_runtime(
    tmp_path: Path,
) -> None:
    """REQ-STO-011: Bootstrap scaffold docs match create_space output."""
    async def _run() -> None:
        layout = _directory_layout()["space_layout"]["bootstrap"]
        entries = layout["entries"]
        if not isinstance(entries, list):
            raise AssertionError("directory-layout bootstrap entries must be a list")

        config = {"uri": f"fs://{tmp_path}"}
        space_id = "doc-sync-space"
        await ugoite_core.create_space(config, space_id)

        space_dir = tmp_path / "spaces" / space_id
        actual_top_level = {path.name for path in space_dir.iterdir()}
        expected_top_level: set[str] = set()
        schemas = _file_schemas()

        for entry in entries:
            if not isinstance(entry, dict):
                raise AssertionError("directory-layout bootstrap entries must be mappings")
            path_template = entry.get("path")
            kind = entry.get("kind")
            if not isinstance(path_template, str) or not isinstance(kind, str):
                raise AssertionError("directory-layout entries require path and kind")
            relative_path = _render_path(path_template, space_id=space_id)
            absolute_path = tmp_path / relative_path
            if kind == "directory":
                if not absolute_path.is_dir():
                    raise AssertionError(f"Missing documented directory: {relative_path}")
            elif kind == "file":
                if not absolute_path.is_file():
                    raise AssertionError(f"Missing documented file: {relative_path}")
                schema_name = entry.get("schema")
                if isinstance(schema_name, str):
                    schema_entry = schemas.get(schema_name)
                    if not isinstance(schema_entry, dict):
                        raise AssertionError(f"Unknown schema: {schema_name}")
                    schema = schema_entry.get("schema")
                    if not isinstance(schema, dict):
                        raise AssertionError(f"Schema {schema_name} must define schema")
                    _validate_schema(
                        _read_json(absolute_path),
                        schema,
                        relative_path.as_posix(),
                    )
            else:
                raise AssertionError(f"Unsupported layout kind: {kind}")

            if absolute_path.parent == space_dir:
                expected_top_level.add(absolute_path.name)

        if actual_top_level != expected_top_level:
            raise AssertionError(
                "Bootstrap scaffold drift: documented top-level entries "
                f"{sorted(expected_top_level)!r} != actual {sorted(actual_top_level)!r}",
            )

    asyncio.run(_run())


def test_docs_req_sto_011_storage_layout_lazy_paths_match_runtime_triggers(
    tmp_path: Path,
) -> None:
    """REQ-STO-011: Lazy storage paths appear only after their documented triggers."""
    async def _run() -> None:
        layout = _directory_layout()["space_layout"]["triggers"]
        if not isinstance(layout, dict):
            raise AssertionError("directory-layout triggers must be a mapping")

        config = {"uri": f"fs://{tmp_path}"}
        space_id = "doc-sync-space"
        await ugoite_core.create_space(config, space_id)

        schemas = _file_schemas()

        response_entries = layout.get("response_signing", {}).get("entries")
        if not isinstance(response_entries, list):
            raise AssertionError("response_signing entries must be a list")
        for entry in response_entries:
            if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
                raise AssertionError("response_signing entries require concrete path")
            before = tmp_path / _render_path(entry["path"], space_id=space_id)
            if before.exists():
                raise AssertionError(
                    f"Lazy response-signing path exists too early: {before}",
                )

        await ugoite_core.load_response_hmac_material(config, space_id)

        for entry in response_entries:
            schema_name = entry.get("schema")
            after = tmp_path / _render_path(str(entry["path"]), space_id=space_id)
            if not after.is_file():
                raise AssertionError(f"Missing response-signing path after trigger: {after}")
            if isinstance(schema_name, str):
                schema_entry = schemas.get(schema_name)
                if not isinstance(schema_entry, dict):
                    raise AssertionError(f"Unknown schema: {schema_name}")
                schema = schema_entry.get("schema")
                if not isinstance(schema, dict):
                    raise AssertionError(f"Schema {schema_name} must define schema")
                _validate_schema(
                    _read_json(after),
                    schema,
                    after.relative_to(tmp_path).as_posix(),
                )

        sql_entries = layout.get("sql_session_creation", {}).get("entries")
        if not isinstance(sql_entries, list):
            raise AssertionError("sql_session_creation entries must be a list")
        materialized_root = tmp_path / "spaces" / space_id / "materialized_views"
        sessions_root = tmp_path / "spaces" / space_id / "sql_sessions"
        if any(materialized_root.iterdir()) or any(sessions_root.iterdir()):
            raise AssertionError(
                "Lazy SQL paths should not exist before create_sql_session",
            )

        session = await ugoite_core.create_sql_session(
            config,
            space_id,
            "SELECT 1 AS value",
        )
        if not isinstance(session, dict):
            raise AssertionError("create_sql_session must return a mapping")
        session_id = str(session.get("id") or "")
        sql_id = str(session.get("sql_id") or "")
        if not session_id or not sql_id:
            raise AssertionError("create_sql_session must return id and sql_id")

        for entry in sql_entries:
            if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
                raise AssertionError("sql_session_creation entries require concrete path")
            relative = _render_path(
                str(entry["path"]),
                space_id=space_id,
                session_id=session_id,
                sql_id=sql_id,
            )
            absolute = tmp_path / relative
            kind = entry.get("kind")
            if kind == "directory" and not absolute.is_dir():
                raise AssertionError(f"Missing documented SQL directory: {relative}")
            if kind == "file":
                if not absolute.is_file():
                    raise AssertionError(f"Missing documented SQL file: {relative}")
                schema_name = entry.get("schema")
                if isinstance(schema_name, str):
                    schema_entry = schemas.get(schema_name)
                    if not isinstance(schema_entry, dict):
                        raise AssertionError(f"Unknown schema: {schema_name}")
                    schema = schema_entry.get("schema")
                    if not isinstance(schema, dict):
                        raise AssertionError(f"Schema {schema_name} must define schema")
                    _validate_schema(_read_json(absolute), schema, relative.as_posix())

    asyncio.run(_run())


def test_docs_req_sto_007_backend_storage_boundary_docs_match_runtime() -> None:
    """REQ-STO-007: Shared-runtime backend docs stay aligned."""
    interface_text = _normalize_whitespace(
        FRONTEND_BACKEND_INTERFACE_PATH.read_text(encoding="utf-8"),
    )
    testing_text = _normalize_whitespace(
        TESTING_STRATEGY_PATH.read_text(encoding="utf-8"),
    )
    requirement_text = _normalize_whitespace(
        STORAGE_REQUIREMENTS_PATH.read_text(encoding="utf-8"),
    )

    details: list[str] = []
    if "must not perform direct filesystem operations" not in interface_text:
        details.append(
            "frontend-backend-interface.md must keep the backend "
            "no-direct-filesystem boundary",
        )
    if (
        "OpenDAL `fs://` (local filesystem) and `memory://` backends"
        not in interface_text
    ):
        details.append(
            "frontend-backend-interface.md must document fs:// and memory:// "
            "backend coverage",
        )
    if "Shared storage abstraction via the `memory://` backend" not in testing_text:
        details.append(
            "testing/strategy.md must describe backend tests through the "
            "shared memory backend",
        )
    if (
        "All runtime filesystem interactions MUST remain inside the shared Rust "
        "storage layer" not in requirement_text
    ):
        details.append(
            "requirements/storage.yaml must keep REQ-STO-007 scoped to the "
            "shared Rust storage layer",
        )
    if details:
        raise AssertionError("; ".join(details))
