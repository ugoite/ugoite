"""Saved SQL management helpers."""

from __future__ import annotations

import json
import uuid
from typing import Any

import ugoite_core

from .utils import (
    run_async,
    split_space_path,
    storage_config_from_root,
    validate_id,
)


def list_sql(space_path: str) -> list[dict[str, Any]]:
    """List saved SQL entries for a space."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path)
    return run_async(ugoite_core.list_sql, config, space_id)


def get_sql(space_path: str, sql_id: str) -> dict[str, Any]:
    """Get a saved SQL entry by ID."""
    validate_id(sql_id, "sql_id")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path)
    return run_async(ugoite_core.get_sql, config, space_id, sql_id)


def create_sql(
    space_path: str,
    payload: dict[str, Any],
    sql_id: str | None = None,
    author: str = "user",
) -> dict[str, Any]:
    """Create a saved SQL entry."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path)
    entry_id = sql_id or str(uuid.uuid4())
    payload_json = json.dumps(payload)
    return run_async(
        ugoite_core.create_sql,
        config,
        space_id,
        entry_id,
        payload_json,
        author,
    )


def update_sql(
    space_path: str,
    sql_id: str,
    payload: dict[str, Any],
    author: str = "user",
) -> dict[str, Any]:
    """Update a saved SQL entry."""
    validate_id(sql_id, "sql_id")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path)
    payload_copy = dict(payload)
    parent_revision_id = payload_copy.pop("parent_revision_id", None)
    payload_json = json.dumps(payload_copy)
    return run_async(
        ugoite_core.update_sql,
        config,
        space_id,
        sql_id,
        payload_json,
        parent_revision_id,
        author,
    )


def delete_sql(space_path: str, sql_id: str) -> None:
    """Delete a saved SQL entry."""
    validate_id(sql_id, "sql_id")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path)
    run_async(ugoite_core.delete_sql, config, space_id, sql_id)
