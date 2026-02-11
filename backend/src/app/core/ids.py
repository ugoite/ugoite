"""Identifier validation helpers."""

from __future__ import annotations

import re
import uuid


def validate_id(identifier: str, name: str) -> str:
    """Validate that an identifier contains only safe characters."""
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        msg = (
            f"Invalid {name}: {identifier}. "
            "Must be alphanumeric, hyphens, or underscores."
        )
        raise ValueError(msg)
    return str(identifier)


def validate_uuid(val: str, name: str) -> str:
    """Validate that value is a valid UUID."""
    try:
        uuid.UUID(val)
        return str(val)
    except ValueError as e:
        msg = f"Invalid {name}: {val}. Must be a valid UUID."
        raise ValueError(msg) from e
