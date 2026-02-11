"""Form management helpers backed by fsspec."""

from __future__ import annotations

import json
import re
from typing import TYPE_CHECKING, Any

import ugoite_core

if TYPE_CHECKING:
    import fsspec


from .utils import (
    fs_exists,
    get_fs_and_path,
    run_async,
    split_space_path,
    storage_config_from_root,
    validate_id,
)


def list_column_types() -> list[str]:
    """Return list of supported column types."""
    return run_async(ugoite_core.list_column_types)


def _space_context(
    space_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    fs_obj, ws_path = get_fs_and_path(space_path, fs)
    if not fs_exists(fs_obj, ws_path):
        msg = f"Space not found: {space_path}"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path


def list_forms(
    space_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return all forms (JSON files) in the space's forms directory."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ugoite_core.list_forms, config, space_id)


def get_form(
    space_path: str,
    form_name: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Return the form definition for ``form_name`` in the space."""
    validate_id(form_name, "form_name")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(ugoite_core.get_form, config, space_id, form_name)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def upsert_form(
    space_path: str,
    form_data: dict[str, Any],
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create or replace a form definition in the space."""
    name = form_data.get("name")
    if not name:
        msg = "Form name is required"
        raise ValueError(msg)
    validate_id(str(name), "form_name")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(form_data)
    run_async(ugoite_core.upsert_form, config, space_id, payload)
    return form_data


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


def migrate_form(
    space_path: str,
    form_data: dict[str, Any],
    strategies: dict[str, Any] | None = None,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> int:
    """Upsert form and migrate existing entries."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(form_data)
    strategies_payload = json.dumps(strategies) if strategies is not None else None
    return run_async(
        ugoite_core.migrate_form,
        config,
        space_id,
        payload,
        strategies_payload,
    )
