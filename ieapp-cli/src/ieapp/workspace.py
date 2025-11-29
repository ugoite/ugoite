"""Workspace management module."""

import base64
import json
import logging
import os
import re
import secrets
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import fsspec
from fsspec.spec import AbstractFileSystem

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

logger = logging.getLogger(__name__)

EMPTY_INDEX_SCHEMA = {"notes": {}, "class_stats": {}}
EMPTY_STATS_SCHEMA = {"last_indexed": 0.0, "note_count": 0, "tag_counts": {}}


class WorkspaceExistsError(Exception):
    """Raised when trying to create a workspace that already exists."""


def _validate_id(identifier: str, name: str) -> None:
    """Validate that the identifier contains only safe characters.

    Args:
        identifier: The string to validate.
        name: The name of the field (for error messages).

    Raises:
        ValueError: If the identifier contains invalid characters.

    """
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        msg = f"Invalid {name}: {identifier}. Must be alphanumeric, hyphens, or underscores."
        raise ValueError(msg)


def _write_json_secure(path: str, payload: dict, mode: int = 0o600) -> None:
    """Write JSON data with restrictive permissions from the start.

    Args:
        path: Absolute file path to write.
        payload: JSON-serializable dictionary.
        mode: File permissions applied at creation time.

    """
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)


def _append_workspace_to_global(global_json_path: str, workspace_id: str) -> None:
    """Append a workspace id to ``global.json`` with advisory locking.

    Args:
        global_json_path: Absolute path to ``global.json``.
        workspace_id: Workspace identifier to append.

    """
    path = Path(global_json_path)
    if not path.exists():
        return

    with path.open("r+", encoding="utf-8") as handle:
        if fcntl:
            fcntl.flock(handle, fcntl.LOCK_EX)

        try:
            try:
                global_data = json.load(handle)
            except json.JSONDecodeError:
                global_data = {"workspaces": []}

            workspaces = global_data.setdefault("workspaces", [])
            if workspace_id not in workspaces:
                workspaces.append(workspace_id)
                handle.seek(0)
                json.dump(global_data, handle, indent=2)
                handle.truncate()
        finally:
            if fcntl:
                fcntl.flock(handle, fcntl.LOCK_UN)


def _ensure_global_json(fs: AbstractFileSystem, root_path_str: str) -> str:
    """Ensure ``global.json`` exists and returns its path.

    Args:
        fs: File system adapter used to write files.
        root_path_str: Absolute path to the IEapp root directory.

    Returns:
        The string path to ``global.json``.

    """
    global_json_path = Path(root_path_str) / "global.json"
    if global_json_path.exists():
        return str(global_json_path)

    now_iso = datetime.now(timezone.utc).isoformat()
    key_id = f"key-{uuid.uuid4().hex}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"file://{Path(root_path_str).resolve()}",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    try:
        fd = os.open(global_json_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2)
    except FileExistsError:
        # Another process created the file; that's fine
        pass
    return str(global_json_path)


def create_workspace(root_path: str | Path, workspace_id: str) -> None:
    """Create a new workspace with the required directory structure and metadata.

    Args:
        root_path: The root directory where workspaces are stored.
        workspace_id: The unique identifier for the workspace.

    Raises:
        WorkspaceExistsError: If the workspace already exists.

    """
    _validate_id(workspace_id, "workspace_id")
    logger.info("Creating workspace %s at %s", workspace_id, root_path)

    # Use fsspec to handle filesystem operations
    # For now, we assume local filesystem if protocol is not specified
    # But fsspec.filesystem("file") works for local

    # If root_path is a string and looks like a URI, parse it.
    # For Milestone 0, we mostly test local paths.

    root_path_str = str(root_path)
    protocol = "file"

    if "://" in root_path_str:
        protocol = root_path_str.split("://")[0]

    if (
        protocol != "file"
        and not root_path_str.startswith("/")
        and not root_path_str.startswith(".")
    ):
        # Simple check, fsspec has better tools but this suffices for M0
        # If it's a windows path it might have :, but usually not ://
        msg = f"Protocol {protocol} not supported in Milestone 0"
        raise NotImplementedError(msg)

    fs = fsspec.filesystem("file")

    # Ensure root exists
    if not fs.exists(root_path_str):
        fs.makedirs(root_path_str)

    global_json_path = _ensure_global_json(fs, root_path_str)

    ws_path = Path(root_path_str) / "workspaces" / workspace_id

    if fs.exists(str(ws_path)):
        msg = f"Workspace {workspace_id} already exists at {ws_path}"
        raise WorkspaceExistsError(msg)

    # Create directories
    dirs = ["schemas", "index", "attachments", "notes"]

    ws_path.mkdir(mode=0o700, parents=True)

    for d in dirs:
        d_path = ws_path / d
        d_path.mkdir(mode=0o700)

    # Create meta.json
    meta = {
        "id": workspace_id,
        "name": workspace_id,
        "created_at": time.time(),
        "storage": {"type": "local", "root": root_path_str},
    }

    meta_path = ws_path / "meta.json"
    _write_json_secure(str(meta_path), meta)

    # Create settings.json
    settings = {"default_class": "Note"}
    settings_path = ws_path / "settings.json"
    _write_json_secure(str(settings_path), settings)

    # Create index/index.json
    index_data = {"notes": {}, "class_stats": {}}
    index_json_path = ws_path / "index" / "index.json"
    _write_json_secure(str(index_json_path), index_data)

    # Create index/stats.json
    stats_data = {
        "last_indexed": time.time(),
        "note_count": 0,
        "tag_counts": {},
    }
    stats_json_path = ws_path / "index" / "stats.json"
    _write_json_secure(str(stats_json_path), stats_data)

    # Update global.json (optional for now, but good practice)
    if fs.exists(global_json_path):
        _append_workspace_to_global(global_json_path, workspace_id)

    logger.info("Workspace %s created successfully", workspace_id)
