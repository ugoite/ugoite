"""Utility functions for ieapp."""

import json
import os
import re
from pathlib import Path
from typing import Any


def safe_resolve_path(base: Path, *parts: str) -> Path:
    """Safely resolve a path ensuring it stays within the base directory.

    Args:
        base: The base directory that the resolved path must be within.
        *parts: Path components to join to the base.

    Returns:
        The resolved absolute path.

    Raises:
        ValueError: If the resolved path would escape the base directory.

    """
    base_resolved = base.resolve()
    target = (base / Path(*parts)).resolve()
    try:
        target.relative_to(base_resolved)
    except ValueError as e:
        msg = f"Path traversal detected: {target} is not within {base_resolved}"
        raise ValueError(msg) from e
    return target


def validate_id(identifier: str, name: str) -> None:
    """Validate that the identifier contains only safe characters.

    Args:
        identifier: The string to validate.
        name: The name of the field (for error messages).

    Raises:
        ValueError: If the identifier contains invalid characters.

    """
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        msg = (
            f"Invalid {name}: {identifier}. "
            "Must be alphanumeric, hyphens, or underscores."
        )
        raise ValueError(msg)


def write_json_secure(
    path: str | Path,
    payload: dict[str, Any],
    mode: int = 0o600,
    *,
    exclusive: bool = False,
) -> None:
    """Write JSON to ``path`` while applying permissions atomically.

    Args:
        path: Target file path.
        payload: JSON-serializable dictionary.
        mode: Permission bits applied at creation.
        exclusive: When True, use ``O_EXCL`` to avoid clobbering existing files.

    """
    flags = os.O_WRONLY | os.O_CREAT
    if exclusive:
        flags |= os.O_EXCL
    else:
        flags |= os.O_TRUNC

    fd = os.open(str(path), flags, mode)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
