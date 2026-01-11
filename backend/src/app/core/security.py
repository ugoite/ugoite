"""Security helpers for the FastAPI application."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

import ieapp

if TYPE_CHECKING:  # pragma: no cover - type hinting helper
    from collections.abc import Mapping
    from pathlib import Path

LOCAL_CLIENT_SENTINELS: Final[set[str]] = {
    "127.0.0.1",
    "localhost",
    "::1",
    "testclient",
    "::ffff:127.0.0.1",
}


def resolve_client_host(
    headers: Mapping[str, str],
    client_host: str | None,
) -> str | None:
    """Resolve the client host honoring proxy headers when present.

    Args:
        headers: Request headers (case-insensitive mapping provided by Starlette).
        client_host: Host extracted from the ASGI scope.

    Returns:
        The best-effort remote address string or ``None`` when unavailable.

    """
    forwarded = headers.get("x-forwarded-for")
    if forwarded:
        candidate = forwarded.split(",", 1)[0].strip()
        if candidate:
            return candidate

    return client_host


def is_local_host(host: str | None) -> bool:
    """Return True when ``host`` represents a loopback address."""
    if host is None:
        return True

    normalized = host.strip().lower()
    if normalized in LOCAL_CLIENT_SENTINELS:
        return True

    return normalized.startswith(("127.", "::ffff:127."))


def build_response_signature(body: bytes, root_path: Path | str) -> tuple[str, str]:
    """Compute the HMAC signature for the response body.

    This function delegates to ieapp-core, which computes the HMAC signature
    using the configured signing key and related settings, keeping
    cryptographic and business logic out of the backend API layer for
    better abstraction and testability.

    Args:
        body: The response body bytes to sign.
        root_path: The root directory where global.json is stored.

    Returns:
        Tuple of (key_id, signature_hex).

    """
    return ieapp.build_response_signature(body, root_path)
