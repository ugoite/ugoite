"""Class management helpers backed by fsspec."""

from __future__ import annotations

import json
import re
from typing import TYPE_CHECKING, Any

import ieapp_core

if TYPE_CHECKING:
    import fsspec


from .utils import (
    fs_exists,
    get_fs_and_path,
    run_async,
    split_workspace_path,
    storage_config_from_root,
    validate_id,
)


def list_column_types() -> list[str]:
    """Return list of supported column types."""
    return run_async(ieapp_core.list_column_types)


def _workspace_context(
    workspace_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    fs_obj, ws_path = get_fs_and_path(workspace_path, fs)
    if not fs_exists(fs_obj, ws_path):
        msg = f"Workspace not found: {workspace_path}"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path


def list_classes(
    workspace_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return all classes (JSON files) in the workspace's classes directory."""
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ieapp_core.list_classes, config, workspace_id)


def get_class(
    workspace_path: str,
    class_name: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Return the class definition for ``class_name`` in the workspace."""
    validate_id(class_name, "class_name")
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(ieapp_core.get_class, config, workspace_id, class_name)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def upsert_class(
    workspace_path: str,
    class_data: dict[str, Any],
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create or replace a class definition in the workspace."""
    name = class_data.get("name")
    if not name:
        msg = "Class name is required"
        raise ValueError(msg)
    validate_id(str(name), "class_name")
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(class_data)
    run_async(ieapp_core.upsert_class, config, workspace_id, payload)
    return class_data


def _apply_migration(markdown: str, strategies: dict[str, Any]) -> str:
    new_markdown = markdown
    for field, strategy in strategies.items():
        if strategy is None:
            # Drop section
            pattern = re.compile(
                rf"^##\s+{re.escape(field)}\s*\n(.*?)(?=(^##|\Z))",
                re.MULTILINE | re.DOTALL,
            )
            new_markdown = pattern.sub("", new_markdown)
        else:
            # Set default using string value
            pattern = re.compile(rf"^##\s+{re.escape(field)}", re.MULTILINE)
            if not pattern.search(new_markdown):
                if not new_markdown.endswith("\n"):
                    new_markdown += "\n"
                new_markdown += f"\n## {field}\n{strategy}\n"

    # Normalize newlines
    return re.sub(r"\n{3,}", "\n\n", new_markdown)


def migrate_class(
    workspace_path: str,
    class_data: dict[str, Any],
    strategies: dict[str, Any] | None = None,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> int:
    """Upsert class and migrate existing notes."""
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(class_data)
    strategies_payload = json.dumps(strategies) if strategies is not None else None
    return run_async(
        ieapp_core.migrate_class,
        config,
        workspace_id,
        payload,
        strategies_payload,
    )
