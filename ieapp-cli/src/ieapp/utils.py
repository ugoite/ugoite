"""Utility functions for ieapp."""

import json
import os
import re
from pathlib import Path
from typing import Any


def resolve_existing_path(base: Path, *parts: str) -> Path:
    """Securely resolve a path to an EXISTING file or directory.

    This function avoids constructing paths from user input by iterating
    directory contents to find matches. This breaks the taint chain for
    static analysis tools like CodeQL.

    Args:
        base: The base directory to start from.
        *parts: Path components to traverse.

    Returns:
        The resolved Path object found in the filesystem.

    Raises:
        FileNotFoundError: If any component does not exist.
        NotADirectoryError: If a component is not a directory when it should be.

    """
    current = base.resolve()
    if not current.exists():
        msg = f"Base path {current} does not exist"
        raise FileNotFoundError(msg)

    for part in parts:
        if not current.is_dir():
            msg = f"{current} is not a directory"
            raise NotADirectoryError(msg)

        # Iterate over directory contents to find the matching child.
        # This ensures the returned path comes from the OS, not user input.
        found = False
        for child in current.iterdir():
            if child.name == part:
                current = child
                found = True
                break

        if not found:
            msg = f"Component {part} not found in {current}"
            raise FileNotFoundError(msg)

    return current


def join_secure_path(base: Path, name: str) -> Path:
    """Securely construct a path for a NEW file or directory.

    This function strictly validates the name component to ensure it
    contains only safe characters, preventing path traversal.

    Args:
        base: The base directory (must be trusted/resolved).
        name: The name of the new file or directory.

    Returns:
        The constructed Path object.

    Raises:
        ValueError: If the name contains invalid characters.

    """
    # Strict allowlist for new filenames
    if not re.match(r"^[a-zA-Z0-9_.-]+$", name):
        msg = (
            f"Invalid filename: {name}. Allowed: alphanumeric, dot, hyphen, underscore."
        )
        raise ValueError(msg)

    # Explicitly reject traversal indicators even if regex somehow missed them
    if ".." in name or "/" in name or "\\" in name:
        msg = f"Invalid filename: {name}"
        raise ValueError(msg)

    return base / name


# Deprecated alias for backward compatibility during refactor,
# but we should move away from it.
safe_resolve_path = resolve_existing_path


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
