import json
import os
import time
import logging
from pathlib import Path
from typing import Union
import fsspec

logger = logging.getLogger(__name__)


class WorkspaceExistsError(Exception):
    """Raised when trying to create a workspace that already exists."""

    pass


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
        # Create global.json if not exists
        global_json_path = os.path.join(root_path_str, "global.json")
        if not fs.exists(global_json_path):
            with fs.open(global_json_path, "w") as f:
                json.dump({"workspaces": [], "created_at": time.time()}, f, indent=2)
            # Set permissions for global.json?
            os.chmod(global_json_path, 0o600)

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

    # Update global.json (optional for now, but good practice)
    global_json_path = os.path.join(root_path_str, "global.json")
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
