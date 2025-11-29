import base64
import json
import os
import secrets
import time
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Union
import fsspec

logger = logging.getLogger(__name__)


class WorkspaceExistsError(Exception):
    """Raised when trying to create a workspace that already exists."""

    pass


def _ensure_global_json(fs, root_path_str: str) -> str:
    """Ensures ``global.json`` exists and returns its path.

    Args:
        fs: File system adapter used to write files.
        root_path_str: Absolute path to the IEapp root directory.

    Returns:
        The string path to ``global.json``.
    """

    global_json_path = os.path.join(root_path_str, "global.json")
    if fs.exists(global_json_path):
        return global_json_path

    now_iso = datetime.now(timezone.utc).isoformat()
    key_id = f"key-{int(time.time())}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"file://{os.path.abspath(root_path_str)}",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    with fs.open(global_json_path, "w") as handle:
        json.dump(payload, handle, indent=2)

    os.chmod(global_json_path, 0o600)
    return global_json_path


def create_workspace(root_path: Union[str, Path], workspace_id: str) -> None:
    """
    Creates a new workspace with the required directory structure and metadata.

    Args:
        root_path: The root directory where workspaces are stored.
        workspace_id: The unique identifier for the workspace.

    Raises:
        WorkspaceExistsError: If the workspace already exists.
    """
    logger.info(f"Creating workspace {workspace_id} at {root_path}")

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
        raise NotImplementedError(f"Protocol {protocol} not supported in Milestone 0")

    fs = fsspec.filesystem("file")

    # Ensure root exists
    if not fs.exists(root_path_str):
        fs.makedirs(root_path_str)

    global_json_path = _ensure_global_json(fs, root_path_str)

    ws_path = os.path.join(root_path_str, "workspaces", workspace_id)

    if fs.exists(ws_path):
        raise WorkspaceExistsError(
            f"Workspace {workspace_id} already exists at {ws_path}"
        )

    # Create directories
    dirs = ["schemas", "index", "attachments", "notes"]

    fs.makedirs(ws_path)
    # Set permissions for workspace dir
    os.chmod(ws_path, 0o700)

    for d in dirs:
        d_path = os.path.join(ws_path, d)
        fs.makedirs(d_path)
        os.chmod(d_path, 0o700)

    # Create meta.json
    meta = {
        "id": workspace_id,
        "name": workspace_id,
        "created_at": time.time(),
        "storage": {"type": "local", "root": root_path_str},
    }

    meta_path = os.path.join(ws_path, "meta.json")
    with fs.open(meta_path, "w") as f:
        json.dump(meta, f, indent=2)
    os.chmod(meta_path, 0o600)

    # Create settings.json
    settings = {"default_class": "Note"}
    settings_path = os.path.join(ws_path, "settings.json")
    with fs.open(settings_path, "w") as f:
        json.dump(settings, f, indent=2)
    os.chmod(settings_path, 0o600)

    # Create index/index.json
    index_data = {"notes": {}, "class_stats": {}}
    index_json_path = os.path.join(ws_path, "index", "index.json")
    with fs.open(index_json_path, "w") as f:
        json.dump(index_data, f, indent=2)
    os.chmod(index_json_path, 0o600)

    # Create index/stats.json
    stats_data = {"last_indexed": time.time(), "note_count": 0, "tag_counts": {}}
    stats_json_path = os.path.join(ws_path, "index", "stats.json")
    with fs.open(stats_json_path, "w") as f:
        json.dump(stats_data, f, indent=2)
    os.chmod(stats_json_path, 0o600)

    # Update global.json (optional for now, but good practice)
    if fs.exists(global_json_path):
        # Read-modify-write (not atomic but fine for M0)
        with fs.open(global_json_path, "r") as f:
            try:
                global_data = json.load(f)
            except json.JSONDecodeError:
                global_data = {"workspaces": []}

        if workspace_id not in global_data.get("workspaces", []):
            if "workspaces" not in global_data:
                global_data["workspaces"] = []
            global_data["workspaces"].append(workspace_id)

            with fs.open(global_json_path, "w") as f:
                json.dump(global_data, f, indent=2)

    logger.info(f"Workspace {workspace_id} created successfully")
