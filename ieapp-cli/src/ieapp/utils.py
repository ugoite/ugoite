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
        *parts: Path components to join to the base. Each component must be a
            single path segment (no path separators).

    Returns:
        The resolved absolute path.

    Raises:
        ValueError: If the resolved path would escape the base directory
                    or if any path component is invalid.

    """
    # Ensure base is resolved
    base_resolved = base.resolve()

    # Start with base
    current_path = base_resolved

    # Validate and join each component
    for part in parts:
        # Reject empty parts, parent directory references, and absolute paths
        if (
            not part
            or part == ".."
            or part.startswith(("/", "\\"))
            or ".." in part.split("/")
            or ".." in part.split("\\")
        ):
            msg = f"Invalid path component: {part}"
            raise ValueError(msg)

        # Use Path(...).name to obtain the final path component (filename).
       # This is more idiomatic with pathlib and avoids os.path.
        safe_part = Path(part).name

        # If 'part' contained separators the name will differ (or it may be empty).
        # Enforce that 'part' is a single path component.
        if safe_part != part or not safe_part:
            msg = f"Invalid path component (must be a single segment): {part}"
            raise ValueError(msg)

        # Join safely using the sanitized component
        current_path = current_path / safe_part

    # Final resolution
    target = current_path.resolve()

    # Final containment check as defense in depth
    try:
        target.relative_to(base_resolved)
    except ValueError as e:
        msg = f"Path traversal detected: {target} is not within {base_resolved}"
        raise ValueError(msg) from e

    return target


def validate_id(identifier: str, name: str) -> str:
    """Validate that an identifier contains only safe characters.

    Returns the sanitized value. This function acts as a security
    sanitizer - it validates the input and returns a safe copy.

    Args:
        identifier: The string to validate.
        name: The name of the field (for error messages).

    Returns:
        The validated identifier (a safe copy).

    Raises:
        ValueError: If the identifier contains invalid characters.

    """
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        msg = (
            f"Invalid {name}: {identifier}. "
            "Must be alphanumeric, hyphens, or underscores."
        )
        raise ValueError(msg)
    # Return a sanitized copy - this breaks the taint chain
    return str(identifier)


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
