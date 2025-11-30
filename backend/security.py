"""Security helpers for the FastAPI application."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import secrets
import uuid
from datetime import UTC, datetime
from functools import lru_cache
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:  # pragma: no cover - type hinting helper
    from collections.abc import Mapping

LOCAL_CLIENT_SENTINELS: Final[set[str]] = {
    "127.0.0.1",
    "localhost",
    "::1",
    "testclient",
    "::ffff:127.0.0.1",
}


def resolve_client_host(
    headers: "Mapping[str, str]",  # noqa: UP037
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


@lru_cache(maxsize=32)
def _load_hmac_material(root_path: str) -> tuple[str, bytes]:
    """Load (or create) the global HMAC key for ``root_path``."""
    root = Path(root_path)
    root.mkdir(parents=True, exist_ok=True)
    global_json = root / "global.json"

    if not global_json.exists():
        _write_default_global(global_json)

    with global_json.open("r", encoding="utf-8") as handle:
        global_data = json.load(handle)

    key_b64 = global_data.get("hmac_key")
    key_id = global_data.get("hmac_key_id", "default")

    if not key_b64:
        msg = "Missing hmac_key in global.json"
        raise ValueError(msg)

    secret = base64.b64decode(key_b64)
    return key_id, secret


def _write_default_global(path: Path) -> None:
    """Write a default global.json with a random HMAC key."""
    now_iso = datetime.now(UTC).isoformat()
    key_id = f"key-{uuid.uuid4().hex}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"file://{path.parent.resolve()}",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    try:
        fd = os.open(path, flags, 0o600)
    except FileExistsError:
        # Another process already created the file between the exists() check
        # and this call. That's fine; the next read will pick it up.
        return

    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)


def build_response_signature(body: bytes, root_path: Path) -> tuple[str, str]:
    """Return the key id + signature for ``body`` relative to ``root_path``."""
    key_id, secret = _load_hmac_material(str(root_path.resolve()))
    digest = hmac.new(secret, body, hashlib.sha256).hexdigest()
    return key_id, digest
