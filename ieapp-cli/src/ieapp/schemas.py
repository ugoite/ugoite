"""Schema management helpers backed by fsspec."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import fsspec


from .indexer import Indexer
from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
    validate_id,
)


def _workspace_context(
    workspace_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    fs_obj, ws_path = get_fs_and_path(workspace_path, fs)
    if not fs_exists(fs_obj, ws_path):
        msg = f"Workspace not found: {workspace_path}"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path


def list_schemas(
    workspace_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return all JSON schemas in the workspace's schemas directory."""
    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    schemas_dir = fs_join(ws_path, "schemas")
    if not fs_exists(fs_obj, schemas_dir):
        return []

    try:
        entries = fs_obj.ls(schemas_dir, detail=False)
    except FileNotFoundError:
        return []

    schemas: list[dict[str, Any]] = []
    for entry in entries:
        path_str = str(entry)
        if not path_str.endswith(".json"):
            continue
        try:
            schemas.append(fs_read_json(fs_obj, path_str))
        except (json.JSONDecodeError, OSError):
            continue
    return schemas


def get_schema(
    workspace_path: str,
    class_name: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Return the JSON schema for ``class_name`` in the workspace."""
    validate_id(class_name, "class_name")
    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    schema_path = fs_join(ws_path, "schemas", f"{class_name}.json")
    if not fs_exists(fs_obj, schema_path):
        msg = f"Schema not found: {class_name}"
        raise FileNotFoundError(msg)
    return fs_read_json(fs_obj, schema_path)


def upsert_schema(
    workspace_path: str,
    schema_data: dict[str, Any],
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create or replace a schema definition in the workspace."""
    name = schema_data.get("name")
    if not name:
        msg = "Schema name is required"
        raise ValueError(msg)
    validate_id(str(name), "schema_name")

    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    schemas_dir = fs_join(ws_path, "schemas")
    fs_makedirs(fs_obj, schemas_dir, exist_ok=True)

    schema_path = fs_join(schemas_dir, f"{name}.json")
    fs_write_json(fs_obj, schema_path, schema_data)

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()

    return schema_data
