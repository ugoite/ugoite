"""Search utilities using fsspec-backed index files."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

import ieapp_core

if TYPE_CHECKING:
    import fsspec

from .utils import (
    fs_exists,
    fs_join,
    fs_read_json,
    get_fs_and_path,
    run_async,
    split_space_path,
    storage_config_from_root,
)


def _space_context(
    space_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    fs_obj, ws_path = get_fs_and_path(space_path, fs)
    if not fs_exists(fs_obj, ws_path):
        msg = f"Space not found: {space_path}"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path


def _load_entries_map(
    fs_obj: fsspec.AbstractFileSystem,
    index_path: str,
) -> dict[str, Any]:
    try:
        index_data = fs_read_json(fs_obj, index_path)
        return index_data.get("entries", {}) if isinstance(index_data, dict) else {}
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


def _search_inverted(
    fs_obj: fsspec.AbstractFileSystem,
    inverted_path: str,
    token: str,
) -> set[str]:
    try:
        inverted = fs_read_json(fs_obj, inverted_path)
    except (FileNotFoundError, json.JSONDecodeError):
        return set()

    matches: set[str] = set()
    for term, entry_ids in inverted.items():
        if token in term:
            matches.update(entry_ids)
    return matches


def _search_index_records(entries_map: dict[str, Any], token: str) -> set[str]:
    matches: set[str] = set()
    for entry_id, record in entries_map.items():
        haystack = json.dumps(record).lower()
        if token in haystack:
            matches.add(entry_id)
    return matches


def _search_content_files(
    fs_obj: fsspec.AbstractFileSystem,
    ws_path: str,
    token: str,
) -> set[str]:
    matches: set[str] = set()
    entries_dir = fs_join(ws_path, "entries")
    if not fs_exists(fs_obj, entries_dir):
        return matches
    try:
        entry_dirs = fs_obj.ls(entries_dir, detail=False)
    except FileNotFoundError:
        return matches

    for entry_dir in entry_dirs:
        content_path = fs_join(entry_dir, "content.json")
        if not fs_exists(fs_obj, content_path):
            continue
        try:
            content_json = fs_read_json(fs_obj, content_path)
        except (json.JSONDecodeError, OSError):
            continue
        if token in json.dumps(content_json).lower():
            matches.add(str(entry_dir).split("/")[-1])
    return matches


def search_entries(
    space_path: str,
    token: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Hybrid keyword search using index and content fallback."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ieapp_core.search_entries, config, space_id, token)
