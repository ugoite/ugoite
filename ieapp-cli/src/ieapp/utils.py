"""Utility functions for ieapp."""

import json
import os
import re
from pathlib import Path
from typing import Any


def safe_resolve_path(base: Path, *parts: str) -> Path:
    """Safely resolve a path ensuring it stays within the base directory.

    This function acts as a security sanitizer for path traversal attacks.
    All path components are validated before being joined.

    Args:
        base: The base directory that the resolved path must be within.
        *parts: Path components to join to the base.

    Returns:
        The resolved absolute path.

    Raises:
        ValueError: If the resolved path would escape the base directory
                    or if any path component is invalid.

    """
    # Validate each path component to ensure no traversal sequences
    for part in parts:
        # Reject path traversal patterns: .., absolute paths, etc.
        if (
            not part
            or part == ".."
            or part.startswith(("/", "\\"))
            or ".." in part.split("/")
            or ".." in part.split("\\")
        ):
            msg = f"Invalid path component: {part}"
            raise ValueError(msg)

    # Construct path using validated components
    base_resolved = base.resolve()
    # Build path step by step with validated components
    # lgtm[py/path-injection] - parts are validated above
    safe_path = base_resolved
    for part in parts:
        safe_path = safe_path / part
    target = safe_path.resolve()

    # Final containment check as defense in depth
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
