"""Workspace management module."""

import base64
import json
import logging
import secrets
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import fsspec
import ieapp_core

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
    run_async,
    storage_config_from_root,
    storage_uri_from_root,
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
    if fs is not None:
        fs_obj, workspaces_dir, workspace_path = _resolve_workspace_paths(
            root_path,
            safe_workspace_id,
            fs=fs,
        )
        if fs_exists(fs_obj, workspace_path):
            msg = f"Workspace {safe_workspace_id} already exists"
            raise WorkspaceExistsError(msg)

        fs_makedirs(fs_obj, workspaces_dir, mode=0o700, exist_ok=True)
        fs_makedirs(fs_obj, workspace_path, mode=0o700, exist_ok=False)

        for directory in ("classes", "index", "attachments", "notes"):
            fs_makedirs(
                fs_obj,
                fs_join(workspace_path, directory),
                mode=0o700,
                exist_ok=True,
            )

        created_at = datetime.now(UTC).timestamp()
        protocol = getattr(fs_obj, "protocol", "file") or "file"
        if isinstance(protocol, (list, tuple)):
            protocol = protocol[0]

        meta_payload = {
            "id": safe_workspace_id,
            "name": safe_workspace_id,
            "created_at": created_at,
            "storage": {
                "type": protocol,
                "root": str(root_path),
            },
        }
        fs_write_json(
            fs_obj,
            fs_join(workspace_path, "meta.json"),
            meta_payload,
            mode=0o600,
            exclusive=True,
        )

        settings_payload = {"default_class": "Note"}
        fs_write_json(
            fs_obj,
            fs_join(workspace_path, "settings.json"),
            settings_payload,
            mode=0o600,
            exclusive=True,
        )

        fs_write_json(
            fs_obj,
            fs_join(workspace_path, "index", "index.json"),
            EMPTY_INDEX_DATA,
            mode=0o600,
            exclusive=True,
        )
        stats_payload = {
            **EMPTY_STATS_DATA,
            "last_indexed": created_at,
        }
        fs_write_json(
            fs_obj,
            fs_join(workspace_path, "index", "stats.json"),
            stats_payload,
            mode=0o600,
            exclusive=True,
        )

        global_json_path = _ensure_global_json(fs_obj, str(root_path))
        _append_workspace_to_global(fs_obj, global_json_path, safe_workspace_id)
        logger.info("Workspace %s created successfully", workspace_id)
        return

    config = storage_config_from_root(root_path, fs)
    try:
        run_async(ieapp_core.create_workspace, config, safe_workspace_id)
    except (RuntimeError, ValueError) as exc:
        msg = str(exc)
        if "already exists" in msg:
            raise WorkspaceExistsError(msg) from exc
        if "scheme is not registered" in msg:
            msg_err = f"Protocol not supported: {msg}"
            raise NotImplementedError(msg_err) from exc
        raise
    logger.info("Workspace %s created successfully", workspace_id)


def get_workspace(
    root_path: str | Path,
    workspace_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Get workspace metadata.

    Args:
        root_path: The root directory where workspaces are stored.
        workspace_id: The unique identifier for the workspace.
        fs: Optional filesystem for non-local storage.

    Returns:
        Dictionary containing workspace metadata.

    Raises:
        FileNotFoundError: If the workspace does not exist.

    """
    if fs is not None:
        fs_obj, _workspaces_dir, workspace_path = _resolve_workspace_paths(
            root_path,
            workspace_id,
            fs=fs,
            must_exist=True,
        )
        meta_path = fs_join(workspace_path, "meta.json")
        try:
            return fs_read_json(fs_obj, meta_path)
        except (json.JSONDecodeError, OSError) as exc:
            msg = f"Invalid workspace metadata: {workspace_id}"
            raise ValueError(msg) from exc

    config = storage_config_from_root(root_path)
    try:
        return run_async(ieapp_core.get_workspace, config, workspace_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def list_workspaces(
    root_path: str | Path,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """List all workspaces.

    Args:
        root_path: The root directory where workspaces are stored.
        fs: Optional filesystem for non-local storage.

    Returns:
        List of workspace metadata dictionaries.

    """
    if fs is not None:
        fs_obj, base_path = get_fs_and_path(root_path, fs)
        global_path = fs_join(base_path, "global.json")
        if not fs_exists(fs_obj, global_path):
            return []
        try:
            global_data = fs_read_json(fs_obj, global_path)
        except (json.JSONDecodeError, OSError):
            return []
        workspace_ids = global_data.get("workspaces", []) or []
        results: list[dict[str, Any]] = []
        for ws_id in workspace_ids:
            try:
                results.append(get_workspace(root_path, str(ws_id), fs=fs_obj))
            except (FileNotFoundError, ValueError):
                continue
        return results

    config = storage_config_from_root(root_path)
    workspace_ids = run_async(ieapp_core.list_workspaces, config)
    results: list[dict[str, Any]] = []
    for ws_id in workspace_ids:
        try:
            results.append(run_async(ieapp_core.get_workspace, config, ws_id))
        except RuntimeError as exc:
            logger.warning("Failed to read workspace meta %s: %s", ws_id, exc)
            continue
    return results


def workspace_path(
    root_path: str | Path,
    workspace_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
    must_exist: bool = False,
) -> str:
    """Public helper returning the absolute workspace path string."""
    if must_exist:
        _ = get_workspace(root_path, workspace_id, fs=fs)
    root_uri = storage_uri_from_root(root_path, fs)
    if root_uri.startswith("fs://") and "://" not in str(root_path):
        return fs_join(str(root_path), "workspaces", workspace_id)
    return f"{root_uri.rstrip('/')}/workspaces/{workspace_id}"


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
    config = storage_config_from_root(root_path, fs)
    patch_payload = patch or {}
    try:
        return run_async(
            ieapp_core.patch_workspace,
            config,
            workspace_id,
            json.dumps(patch_payload),
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def test_storage_connection(storage_config: dict[str, Any]) -> dict[str, object]:
    """Validate storage connector payload (stub for now)."""
    return run_async(ieapp_core.test_storage_connection, storage_config)
