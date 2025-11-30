"""Configuration settings."""

import os
from pathlib import Path


def get_root_path() -> Path:
    """Get the root path for workspaces."""
    return Path(os.environ.get("IEAPP_ROOT", str(Path.cwd())))
