"""Storage documentation consistency tests.

REQ-STO-001: Storage docs must declare OpenDAL as the current runtime layer.
REQ-STO-007: Backend/core storage boundary docs must match the shared runtime.
REQ-STO-011: Storage layout docs must match runtime-created paths and schemas.
"""

from __future__ import annotations

import asyncio
import json
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING, Never

import ugoite_core
import yaml

if TYPE_CHECKING:
    from collections.abc import Callable

JSONScalar = None | bool | int | float | str
JSONValue = JSONScalar | list["JSONValue"] | dict[str, "JSONValue"]
SchemaMap = dict[str, dict[str, object]]

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


def _fail(message: str) -> Never:
    raise AssertionError(message)


def _normalize_whitespace(text: str) -> str:
    return " ".join(text.split())


def _require_mapping(value: object, context: str) -> dict[str, object]:
    if not isinstance(value, dict):
        _fail(f"{context} must be a mapping")
    return value


def _require_list(value: object, context: str) -> list[object]:
    if not isinstance(value, list):
        _fail(f"{context} must be a list")
    return value


def _require_string(value: object, context: str) -> str:
    if not isinstance(value, str):
        _fail(f"{context} must be a string")
    return value


def _load_yaml_mapping(path: Path) -> dict[str, object]:
    loaded = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(loaded, dict):
        relative_path = path.relative_to(REPO_ROOT)
        _fail(f"{relative_path} must be a YAML mapping")
    return loaded


def _directory_layout() -> dict[str, object]:
    return _load_yaml_mapping(DIRECTORY_LAYOUT_PATH)


def _file_schemas() -> SchemaMap:
    raw_schemas = _load_yaml_mapping(FILE_SCHEMAS_PATH).get("schemas")
    schemas = _require_mapping(raw_schemas, "file-schemas.yaml schemas")
    normalized: SchemaMap = {}
    for name, schema in schemas.items():
        if not isinstance(name, str):
            _fail("file-schemas.yaml schema names must be strings")
        normalized[name] = _require_mapping(schema, f"file-schemas.yaml schema {name}")
    return normalized


def _render_path(template: str, **values: str) -> Path:
    return Path(template.format(**values))


def _read_json(path: Path) -> JSONValue:
    return json.loads(path.read_text(encoding="utf-8"))


async def _create_space(config: dict[str, str], space_id: str) -> None:
    await ugoite_core.create_space(config, space_id)


async def _load_response_hmac_material(config: dict[str, str], space_id: str) -> None:
    await ugoite_core.load_response_hmac_material(config, space_id)


async def _create_sql_session(
    config: dict[str, str],
    space_id: str,
    sql: str,
) -> dict[str, object]:
    session = await ugoite_core.create_sql_session(config, space_id, sql)
    return _require_mapping(session, "create_sql_session return value")


def _is_json_integer(value: JSONValue) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _is_json_number(value: JSONValue) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _schema_predicates() -> dict[str, Callable[[JSONValue], bool]]:
    return {
        "array": lambda value: isinstance(value, list),
        "boolean": lambda value: isinstance(value, bool),
        "integer": _is_json_integer,
        "null": lambda value: value is None,
        "number": _is_json_number,
        "object": lambda value: isinstance(value, dict),
        "string": lambda value: isinstance(value, str),
    }


def _matches_schema_type(value: JSONValue, expected: str) -> bool:
    predicate = _schema_predicates().get(expected)
    if predicate is None:
        _fail(f"Unsupported schema type: {expected}")
    return predicate(value)


def _validate_format(value: JSONValue, expected_format: str, context: str) -> None:
    if value is None or expected_format != "date-time":
        return
    if not isinstance(value, str):
        _fail(f"{context} must be a date-time string")
    try:
        datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:  # pragma: no cover - exercised on failure
        message = f"{context} must be valid RFC3339: {value}"
        raise AssertionError(message) from exc


def _validate_declared_type(
    value: JSONValue,
    schema: dict[str, object],
    context: str,
) -> object:
    schema_type = schema.get("type")
    if isinstance(schema_type, list):
        option_names = [str(option) for option in schema_type]
        if not any(_matches_schema_type(value, option) for option in option_names):
            _fail(f"{context} must match one of {option_names!r}")
        return schema_type
    if isinstance(schema_type, str) and not _matches_schema_type(value, schema_type):
        _fail(f"{context} must be {schema_type}")
    return schema_type


def _validate_enum(value: JSONValue, schema: dict[str, object], context: str) -> None:
    enum_values = schema.get("enum")
    if isinstance(enum_values, list) and value not in enum_values:
        _fail(f"{context} must be one of {enum_values!r}")


def _validate_required_keys(
    value: dict[str, JSONValue],
    schema: dict[str, object],
    context: str,
) -> None:
    required_values = schema.get("required", [])
    required_keys = _require_list(required_values, f"{context} required keys")
    missing = [str(key) for key in required_keys if key not in value]
    if missing:
        _fail(f"{context} missing required keys: {missing!r}")


def _validate_properties(
    value: dict[str, JSONValue],
    schema: dict[str, object],
    context: str,
) -> None:
    properties_value = schema.get("properties", {})
    properties = _require_mapping(properties_value, f"{context} properties")
    for key, property_schema in properties.items():
        if key not in value:
            continue
        property_name = _require_string(key, f"{context} property name")
        _validate_schema(
            value[property_name],
            _require_mapping(property_schema, f"{context}.{property_name} schema"),
            f"{context}.{property_name}",
        )


def _validate_additional_properties(
    value: dict[str, JSONValue],
    schema: dict[str, object],
    context: str,
) -> None:
    additional = schema.get("additionalProperties")
    if not isinstance(additional, dict):
        return
    properties_value = schema.get("properties", {})
    properties = _require_mapping(properties_value, f"{context} properties")
    handled = {str(key) for key in properties}
    for key, item in value.items():
        if key in handled:
            continue
        _validate_schema(item, additional, f"{context}.{key}")


def _validate_object_schema(
    value: JSONValue,
    schema: dict[str, object],
    context: str,
) -> None:
    object_value = _require_mapping(value, context)
    _validate_required_keys(object_value, schema, context)
    _validate_properties(object_value, schema, context)
    _validate_additional_properties(object_value, schema, context)


def _validate_array_schema(
    value: JSONValue,
    schema: dict[str, object],
    context: str,
) -> None:
    array_value = _require_list(value, context)
    item_schema = schema.get("items")
    if not isinstance(item_schema, dict):
        return
    for index, item in enumerate(array_value):
        _validate_schema(item, item_schema, f"{context}[{index}]")


def _validate_schema(value: JSONValue, schema: dict[str, object], context: str) -> None:
    schema_type = _validate_declared_type(value, schema, context)
    _validate_enum(value, schema, context)
    expected_format = schema.get("format")
    if isinstance(expected_format, str):
        _validate_format(value, expected_format, context)
    if schema_type == "object":
        _validate_object_schema(value, schema, context)
    if schema_type == "array":
        _validate_array_schema(value, schema, context)


def _space_layout() -> dict[str, object]:
    return _require_mapping(
        _directory_layout().get("space_layout"),
        "directory-layout.yaml space_layout",
    )


def _entry_mappings(entries: object, context: str) -> list[dict[str, object]]:
    return [
        _require_mapping(entry, context) for entry in _require_list(entries, context)
    ]


def _bootstrap_entries() -> list[dict[str, object]]:
    bootstrap = _require_mapping(
        _space_layout().get("bootstrap"),
        "directory-layout.yaml bootstrap",
    )
    return _entry_mappings(
        bootstrap.get("entries"),
        "directory-layout bootstrap entries",
    )


def _trigger_entries(trigger_name: str) -> list[dict[str, object]]:
    triggers = _require_mapping(
        _space_layout().get("triggers"),
        "directory-layout.yaml triggers",
    )
    trigger = _require_mapping(
        triggers.get(trigger_name),
        f"directory-layout.yaml trigger {trigger_name}",
    )
    return _entry_mappings(trigger.get("entries"), f"{trigger_name} entries")


def _schema_definition(schemas: SchemaMap, schema_name: str) -> dict[str, object]:
    schema_entry = schemas.get(schema_name)
    if schema_entry is None:
        _fail(f"Unknown schema: {schema_name}")
    return _require_mapping(schema_entry.get("schema"), f"Schema {schema_name}")


def _entry_path(entry: dict[str, object], context: str, **values: str) -> Path:
    template = _require_string(entry.get("path"), f"{context} path")
    return _render_path(template, **values)


def _entry_kind(entry: dict[str, object], context: str) -> str:
    return _require_string(entry.get("kind"), f"{context} kind")


def _validate_documented_file(
    entry: dict[str, object],
    path: Path,
    schemas: SchemaMap,
    context: str,
) -> None:
    if not path.is_file():
        _fail(f"Missing documented file: {path}")
    schema_name = entry.get("schema")
    if isinstance(schema_name, str):
        schema = _schema_definition(schemas, schema_name)
        _validate_schema(_read_json(path), schema, context)


def _validate_bootstrap_entry(
    entry: dict[str, object],
    tmp_path: Path,
    space_dir: Path,
    space_id: str,
    schemas: SchemaMap,
) -> str | None:
    relative_path = _entry_path(
        entry,
        "directory-layout bootstrap entry",
        space_id=space_id,
    )
    absolute_path = tmp_path / relative_path
    kind = _entry_kind(entry, "directory-layout bootstrap entry")
    if kind == "directory":
        if not absolute_path.is_dir():
            _fail(f"Missing documented directory: {relative_path}")
    elif kind == "file":
        _validate_documented_file(
            entry,
            absolute_path,
            schemas,
            relative_path.as_posix(),
        )
    else:
        _fail(f"Unsupported layout kind: {kind}")

    if absolute_path.parent == space_dir:
        return absolute_path.name
    return None


def _validate_bootstrap_layout(
    tmp_path: Path,
    space_id: str,
    schemas: SchemaMap,
) -> None:
    space_dir = tmp_path / "spaces" / space_id
    actual_top_level = {path.name for path in space_dir.iterdir()}
    expected_top_level = {
        name
        for name in (
            _validate_bootstrap_entry(entry, tmp_path, space_dir, space_id, schemas)
            for entry in _bootstrap_entries()
        )
        if name is not None
    }
    if actual_top_level != expected_top_level:
        actual_names = sorted(actual_top_level)
        expected_names = sorted(expected_top_level)
        _fail(
            "Bootstrap scaffold drift: "
            "documented top-level entries "
            f"{expected_names!r} != actual {actual_names!r}",
        )


def _validate_response_signing_trigger(
    tmp_path: Path,
    space_id: str,
    schemas: SchemaMap,
) -> None:
    entries = _trigger_entries("response_signing")
    for entry in entries:
        before = tmp_path / _entry_path(
            entry,
            "response_signing entry",
            space_id=space_id,
        )
        if before.exists():
            _fail(f"Lazy response-signing path exists too early: {before}")

    config = {"uri": f"fs://{tmp_path}"}
    asyncio.run(_load_response_hmac_material(config, space_id))

    for entry in entries:
        after = tmp_path / _entry_path(
            entry,
            "response_signing entry",
            space_id=space_id,
        )
        _validate_documented_file(
            entry,
            after,
            schemas,
            after.relative_to(tmp_path).as_posix(),
        )


def _load_sql_session_ids(
    tmp_path: Path,
    space_id: str,
) -> tuple[str, str]:
    config = {"uri": f"fs://{tmp_path}"}
    session_mapping = asyncio.run(
        _create_sql_session(config, space_id, "SELECT 1 AS value"),
    )
    session_id = _require_string(session_mapping.get("id"), "create_sql_session id")
    sql_id = _require_string(session_mapping.get("sql_id"), "create_sql_session sql_id")
    return session_id, sql_id


def _assert_sql_roots_start_empty(tmp_path: Path, space_id: str) -> None:
    materialized_root = tmp_path / "spaces" / space_id / "materialized_views"
    sessions_root = tmp_path / "spaces" / space_id / "sql_sessions"
    if any(materialized_root.iterdir()) or any(sessions_root.iterdir()):
        _fail("Lazy SQL paths should not exist before create_sql_session")


def _validate_sql_trigger_entry(
    entry: dict[str, object],
    tmp_path: Path,
    path_values: dict[str, str],
    schemas: SchemaMap,
) -> None:
    relative_path = _entry_path(
        entry,
        "sql_session_creation entry",
        **path_values,
    )
    absolute_path = tmp_path / relative_path
    kind = _entry_kind(entry, "sql_session_creation entry")
    if kind == "directory":
        if not absolute_path.is_dir():
            _fail(f"Missing documented SQL directory: {relative_path}")
        return
    if kind != "file":
        _fail(f"Unsupported layout kind: {kind}")
    _validate_documented_file(entry, absolute_path, schemas, relative_path.as_posix())


def _validate_sql_session_trigger(
    tmp_path: Path,
    space_id: str,
    schemas: SchemaMap,
) -> None:
    _assert_sql_roots_start_empty(tmp_path, space_id)
    session_id, sql_id = _load_sql_session_ids(tmp_path, space_id)
    path_values = {
        "space_id": space_id,
        "session_id": session_id,
        "sql_id": sql_id,
    }
    for entry in _trigger_entries("sql_session_creation"):
        _validate_sql_trigger_entry(
            entry,
            tmp_path,
            path_values,
            schemas,
        )


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
        _fail("; ".join(details))


def test_docs_req_sto_011_storage_layout_bootstrap_matches_runtime(
    tmp_path: Path,
) -> None:
    """REQ-STO-011: Bootstrap scaffold docs match create_space output."""
    config = {"uri": f"fs://{tmp_path}"}
    space_id = "doc-sync-space"
    asyncio.run(_create_space(config, space_id))
    _validate_bootstrap_layout(tmp_path, space_id, _file_schemas())


def test_docs_req_sto_011_storage_layout_lazy_paths_match_runtime_triggers(
    tmp_path: Path,
) -> None:
    """REQ-STO-011: Lazy storage paths appear only after their documented triggers."""
    config = {"uri": f"fs://{tmp_path}"}
    space_id = "doc-sync-space"
    asyncio.run(_create_space(config, space_id))
    schemas = _file_schemas()
    _validate_response_signing_trigger(tmp_path, space_id, schemas)
    _validate_sql_session_trigger(tmp_path, space_id, schemas)


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
        _fail("; ".join(details))
