"""Workspace management module."""

import base64
import json
import logging
import secrets
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import fsspec

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
    validate_id,
)

logger = logging.getLogger(__name__)

EMPTY_INDEX_DATA = {"notes": {}, "class_stats": {}}
EMPTY_STATS_DATA = {"last_indexed": 0.0, "note_count": 0, "tag_counts": {}}


def _resolve_workspace_paths(
    root_path: str | Path,
    workspace_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
    must_exist: bool = False,
) -> tuple[fsspec.AbstractFileSystem, str, str]:
    """Return filesystem, workspaces dir, and workspace path strings."""
    safe_workspace_id = validate_id(workspace_id, "workspace_id")
    try:
        fs_obj, base_path = get_fs_and_path(root_path, fs)
    except (ImportError, ValueError) as exc:
        msg = "Protocol not supported in current runtime"
        raise NotImplementedError(msg) from exc
    workspaces_dir = fs_join(base_path, "workspaces")
    workspace_path = fs_join(workspaces_dir, safe_workspace_id)

    if must_exist and not fs_exists(fs_obj, workspace_path):
        msg = f"Workspace {safe_workspace_id} not found"
        raise FileNotFoundError(msg)

    return fs_obj, workspaces_dir, workspace_path


class WorkspaceExistsError(Exception):
    """Raised when trying to create a workspace that already exists."""


def _append_workspace_to_global(
    fs: fsspec.AbstractFileSystem,
    global_json_path: str,
    workspace_id: str,
) -> None:
    """Append a workspace id to ``global.json`` using fsspec."""
    if not fs_exists(fs, global_json_path):
        return

    try:
        global_data = fs_read_json(fs, global_json_path)
    except (json.JSONDecodeError, OSError):
        global_data = {"workspaces": []}

    workspaces = global_data.setdefault("workspaces", [])
    if workspace_id in workspaces:
        return

    workspaces.append(workspace_id)
    fs_write_json(fs, global_json_path, global_data)


def _ensure_global_json(fs: fsspec.AbstractFileSystem, root_path: str) -> str:
    """Ensure ``global.json`` exists under ``root_path`` using fsspec."""
    global_json_path = fs_join(root_path, "global.json")
    if fs_exists(fs, global_json_path):
        return global_json_path

    protocol = getattr(fs, "protocol", "file") or "file"
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    now_iso = datetime.now(UTC).isoformat()
    key_id = f"key-{uuid.uuid4().hex}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"{protocol}://{root_path}",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    fs_write_json(fs, global_json_path, payload, mode=0o600, exclusive=True)
    return global_json_path


def create_workspace(
    root_path: str | Path,
    workspace_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Create a new workspace with the required directory structure."""
    safe_workspace_id = validate_id(workspace_id, "workspace_id")
    try:
        fs_obj, base_path = get_fs_and_path(root_path, fs)
    except (ImportError, ValueError) as exc:
        msg = "Protocol not supported in current runtime"
        raise NotImplementedError(msg) from exc
    logger.info("Creating workspace %s at %s", safe_workspace_id, base_path)

    fs_makedirs(fs_obj, base_path, exist_ok=True)

    workspaces_dir = fs_join(base_path, "workspaces")
    fs_makedirs(fs_obj, workspaces_dir, exist_ok=True)

    ws_path = fs_join(workspaces_dir, safe_workspace_id)
    if fs_exists(fs_obj, ws_path):
        msg = f"Workspace {safe_workspace_id} already exists at {ws_path}"
        raise WorkspaceExistsError(msg)

    fs_makedirs(fs_obj, ws_path, exist_ok=False)

    for subdir in ["classes", "index", "attachments", "notes"]:
        fs_makedirs(fs_obj, fs_join(ws_path, subdir), exist_ok=False)

    meta = {
        "id": safe_workspace_id,
        "name": safe_workspace_id,
        "created_at": time.time(),
        "storage": {"type": "local", "root": base_path},
    }
    fs_write_json(fs_obj, fs_join(ws_path, "meta.json"), meta)

    settings = {"default_class": "Note"}
    fs_write_json(fs_obj, fs_join(ws_path, "settings.json"), settings)

    index_data = {"notes": {}, "class_stats": {}}
    fs_write_json(fs_obj, fs_join(ws_path, "index/index.json"), index_data)

    stats_data = {
        "last_indexed": time.time(),
        "note_count": 0,
        "tag_counts": {},
    }
    fs_write_json(fs_obj, fs_join(ws_path, "index/stats.json"), stats_data)

    global_json_path = _ensure_global_json(fs_obj, base_path)
    _append_workspace_to_global(fs_obj, global_json_path, safe_workspace_id)

    logger.info("Workspace %s created successfully", workspace_id)


def get_workspace(root_path: str | Path, workspace_id: str) -> dict[str, Any]:
    """Get workspace metadata.

    Args:
        root_path: The root directory where workspaces are stored.
        workspace_id: The unique identifier for the workspace.

    Returns:
        Dictionary containing workspace metadata.

    Raises:
        FileNotFoundError: If the workspace does not exist.

    """
    fs_obj, _workspaces_dir, ws_path = _resolve_workspace_paths(
        root_path,
        workspace_id,
        must_exist=True,
    )

    meta_path = fs_join(ws_path, "meta.json")
    if not fs_exists(fs_obj, meta_path):
        msg = f"Workspace {workspace_id} metadata not found"
        raise FileNotFoundError(msg)

    return fs_read_json(fs_obj, meta_path)


def list_workspaces(root_path: str | Path) -> list[dict[str, Any]]:
    """List all workspaces.

    Args:
        root_path: The root directory where workspaces are stored.

    Returns:
        List of workspace metadata dictionaries.

    """
    fs_obj, base_path = get_fs_and_path(root_path)
    workspaces_dir = fs_join(base_path, "workspaces")

    if not fs_exists(fs_obj, workspaces_dir):
        return []

    workspaces: list[dict[str, Any]] = []
    try:
        ws_entries = fs_obj.ls(workspaces_dir, detail=True)
    except FileNotFoundError:
        return []

    for entry in ws_entries:
        if isinstance(entry, dict):
            if entry.get("type") != "directory":
                continue
            ws_path = entry.get("name") or entry.get("path") or ""
        else:
            ws_path = str(entry)

        meta_path = fs_join(ws_path, "meta.json")
        if not fs_exists(fs_obj, meta_path):
            continue

        try:
            workspaces.append(fs_read_json(fs_obj, meta_path))
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Failed to read workspace meta %s: %s", meta_path, exc)
            continue

    return workspaces


def workspace_path(
    root_path: str | Path,
    workspace_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
    must_exist: bool = False,
) -> str:
    """Public helper returning the absolute workspace path string."""
    fs_obj, _, ws_path = _resolve_workspace_paths(
        root_path,
        workspace_id,
        fs=fs,
        must_exist=must_exist,
    )

    # If we are using a non-local filesystem, we must return a URI
    # so that subsequent calls (like create_note) can resolve the correct filesystem.
    protocol = getattr(fs_obj, "protocol", "file")
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    if protocol != "file":
        # Reconstruct URI: protocol://path
        # Ensure path doesn't start with / if we are appending to protocol://
        return f"{protocol}://{ws_path.lstrip('/')}"

    return ws_path


def patch_workspace(
    root_path: str | Path,
    workspace_id: str,
    *,
    patch: dict[str, Any] | None = None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Update workspace metadata and settings using fsspec.

    The `patch` dict may contain keys: ``name``, ``storage_config``, and ``settings``.
    """
    fs_obj, _workspaces_dir, ws_path = _resolve_workspace_paths(
        root_path,
        workspace_id,
        fs=fs,
        must_exist=True,
    )

    if patch is None:
        patch = {}

    name = patch.get("name") if isinstance(patch, dict) else None
    storage_config = patch.get("storage_config") if isinstance(patch, dict) else None
    settings = patch.get("settings") if isinstance(patch, dict) else None

    meta_path = fs_join(ws_path, "meta.json")
    settings_path = fs_join(ws_path, "settings.json")

    if not fs_exists(fs_obj, meta_path):
        msg = f"Workspace {workspace_id} not found"
        raise FileNotFoundError(msg)

    meta = fs_read_json(fs_obj, meta_path)
    if fs_exists(fs_obj, settings_path):
        current_settings = fs_read_json(fs_obj, settings_path)
    else:
        current_settings = {}

    if name:
        meta["name"] = name
    if storage_config:
        meta["storage_config"] = storage_config
    if settings:
        current_settings.update(settings)

    fs_write_json(fs_obj, meta_path, meta)
    fs_write_json(fs_obj, settings_path, current_settings)

    return {**meta, "settings": current_settings}


def test_storage_connection(storage_config: dict[str, Any]) -> dict[str, str]:
    """Validate storage connector payload (stub for now)."""
    uri = storage_config.get("uri", "") if isinstance(storage_config, dict) else ""

    if uri.startswith(("file://", "/", ".")):
        return {"status": "ok", "mode": "local"}

    if uri.startswith("s3://"):
        return {"status": "ok", "mode": "s3"}

    msg = "Unsupported storage connector"
    raise ValueError(msg)
