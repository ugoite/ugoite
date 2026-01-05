"""Configuration settings."""

import os
from pathlib import Path


def get_root_path() -> str | Path:
    """Get the root path for workspaces."""
    root = os.environ.get("IEAPP_ROOT")
    if root:
        return root
    return Path.cwd()
