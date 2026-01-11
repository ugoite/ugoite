"""Class management helpers backed by fsspec."""

from __future__ import annotations

import json
import re
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import fsspec


from .indexer import Indexer, extract_properties
from .notes import get_note, list_notes, update_note
from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
    validate_id,
)


def list_column_types() -> list[str]:
    """Return list of supported column types."""
    return ["string", "number", "date", "list", "markdown"]


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
    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    classes_dir = fs_join(ws_path, "classes")
    if not fs_exists(fs_obj, classes_dir):
        return []

    try:
        entries = fs_obj.ls(classes_dir, detail=False)
    except FileNotFoundError:
        return []

    classes: list[dict[str, Any]] = []
    for entry in entries:
        path_str = str(entry)
        if not path_str.endswith(".json"):
            continue
        try:
            classes.append(fs_read_json(fs_obj, path_str))
        except (json.JSONDecodeError, OSError):
            continue
    return classes


def get_class(
    workspace_path: str,
    class_name: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Return the class definition for ``class_name`` in the workspace."""
    validate_id(class_name, "class_name")
    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    class_path = fs_join(ws_path, "classes", f"{class_name}.json")
    if not fs_exists(fs_obj, class_path):
        msg = f"Class not found: {class_name}"
        raise FileNotFoundError(msg)
    return fs_read_json(fs_obj, class_path)


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

    fs_obj, ws_path = _workspace_context(workspace_path, fs)
    classes_dir = fs_join(ws_path, "classes")
    fs_makedirs(fs_obj, classes_dir, exist_ok=True)

    class_path = fs_join(classes_dir, f"{name}.json")
    fs_write_json(fs_obj, class_path, class_data)

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()

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
    upsert_class(workspace_path, class_data, fs=fs)

    if not strategies:
        return 0

    fs_obj, _ws_path = _workspace_context(workspace_path, fs)
    class_name = class_data.get("name")
    if not class_name:
        return 0

    all_notes = list_notes(workspace_path, fs=fs_obj)
    updated_count = 0

    for note_summary in all_notes:
        note_id = note_summary["id"]
        try:
            note = get_note(workspace_path, note_id, fs=fs_obj)
        except FileNotFoundError:
            continue

        props = extract_properties(note["content"])
        if props.get("class") != class_name:
            continue

        original_md = note["content"]
        new_md = _apply_migration(original_md, strategies)

        if new_md != original_md:
            update_note(
                workspace_path,
                note_id,
                content=new_md,
                author="system-migration",
                parent_revision_id=note["revision_id"],
                fs=fs_obj,
            )
            updated_count += 1

    return updated_count
