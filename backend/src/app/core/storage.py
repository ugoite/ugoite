"""Storage helpers for ieapp-core integration."""

from __future__ import annotations

from pathlib import Path


def storage_uri_from_root(root_path: Path | str) -> str:
    """Return an OpenDAL-compatible storage URI for the workspace root."""
    root_str = str(root_path)
    if "://" in root_str:
        return root_str
    return f"fs://{root_str}"


def storage_config_from_root(root_path: Path | str) -> dict[str, str]:
    """Build storage_config for ieapp-core bindings."""
    return {"uri": storage_uri_from_root(root_path)}


def workspace_uri(root_path: Path | str, workspace_id: str) -> str:
    """Build a workspace URI/path for API responses."""
    root_uri = storage_uri_from_root(root_path)
    if root_uri.startswith("fs://"):
        base = root_uri[len("fs://") :]
        return str(Path(base) / "workspaces" / workspace_id)
    return f"{root_uri.rstrip('/')}/workspaces/{workspace_id}"
