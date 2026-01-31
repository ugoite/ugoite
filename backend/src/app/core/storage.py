"""Storage helpers for ieapp-core integration."""

from __future__ import annotations

import logging
from pathlib import Path
from urllib.parse import unquote, urlparse

logger = logging.getLogger(__name__)

LOCAL_STORAGE_PATH_EMPTY_ERROR = "Local storage path is empty"


def _ensure_local_root(root_path: Path | str) -> None:
    """Ensure the local workspace root directory exists."""
    root_str = str(root_path)
    parsed = urlparse(root_str)
    if parsed.scheme:
        if parsed.scheme in {"file", "fs"}:
            local_path = Path(unquote(parsed.path))
            if not str(local_path):
                raise ValueError(LOCAL_STORAGE_PATH_EMPTY_ERROR)
            try:
                local_path.mkdir(parents=True, exist_ok=True)
            except OSError:
                logger.exception("Failed to create local storage root: %s", local_path)
                raise
        return
    try:
        Path(root_str).mkdir(parents=True, exist_ok=True)
    except OSError:
        logger.exception("Failed to create local storage root: %s", root_str)
        raise


def storage_uri_from_root(root_path: Path | str) -> str:
    """Return an OpenDAL-compatible storage URI for the workspace root."""
    root_str = str(root_path)
    if "://" in root_str:
        return root_str
    return f"fs://{root_str}"


def storage_config_from_root(root_path: Path | str) -> dict[str, str]:
    """Build storage_config for ieapp-core bindings."""
    _ensure_local_root(root_path)
    return {"uri": storage_uri_from_root(root_path)}


def workspace_uri(root_path: Path | str, workspace_id: str) -> str:
    """Build a workspace URI/path for API responses."""
    root_uri = storage_uri_from_root(root_path)
    if root_uri.startswith("fs://"):
        base = root_uri[len("fs://") :]
        return str(Path(base) / "workspaces" / workspace_id)
    return f"{root_uri.rstrip('/')}/workspaces/{workspace_id}"
