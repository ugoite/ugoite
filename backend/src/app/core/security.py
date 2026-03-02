"""Security helpers for the FastAPI application."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import secrets
import time
import re
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Final

import anyio

if TYPE_CHECKING:  # pragma: no cover - type hinting helper
    from collections.abc import Mapping

LOCAL_CLIENT_SENTINELS: Final[set[str]] = {
    "127.0.0.1",
    "localhost",
    "::1",
    "testclient",
    "::ffff:127.0.0.1",
}
_SPACE_ID_PATTERN: Final[re.Pattern[str]] = re.compile(r"^[A-Za-z0-9_-]+$")


def resolve_client_host(
    headers: Mapping[str, str],
    client_host: str | None,
    *,
    trust_proxy_headers: bool = False,
) -> str | None:
    """Resolve the client host honoring proxy headers when present.

    Args:
        headers: Request headers (case-insensitive mapping provided by Starlette).
        client_host: Host extracted from the ASGI scope.
        trust_proxy_headers: Whether to honor `X-Forwarded-For` from trusted proxies.

    Returns:
        The best-effort remote address string or ``None`` when unavailable.

    """
    if trust_proxy_headers:
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


def _validated_space_id(space_id: str) -> str:
    normalized = space_id.strip()
    if not normalized or _SPACE_ID_PATTERN.fullmatch(normalized) is None:
        msg = f"Invalid space_id for HMAC material: {space_id!r}"
        raise ValueError(msg)
    return normalized


def _space_hmac_path(root_path: Path | str, space_id: str) -> Path:
    spaces_root = (Path(root_path) / "spaces").resolve()
    space_dir = (spaces_root / _validated_space_id(space_id)).resolve()
    try:
        space_dir.relative_to(spaces_root)
    except ValueError as exc:
        msg = f"Resolved space path escapes root: {space_dir}"
        raise ValueError(msg) from exc
    return space_dir / "hmac.json"


def _initialize_response_hmac_material(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "hmac_key_id": f"key-{uuid.uuid4().hex}",
        "hmac_key": base64.b64encode(secrets.token_bytes(32)).decode("ascii"),
        "last_rotation": datetime.now(UTC).isoformat(),
    }
    serialized = json.dumps(payload, indent=2)
    try:
        with path.open("x", encoding="utf-8") as stream:
            stream.write(serialized)
    except FileExistsError:
        return
    if os.name != "nt":
        path.chmod(0o600)


def _load_response_hmac_material(
    root_path: Path | str,
    space_id: str,
) -> tuple[str, bytes]:
    path = _space_hmac_path(root_path, space_id)
    _initialize_response_hmac_material(path)
    payload: dict[str, object] | None = None
    for _ in range(3):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            break
        except json.JSONDecodeError:
            time.sleep(0.01)
    if payload is None:
        payload = json.loads(path.read_text(encoding="utf-8"))

    key_b64 = payload.get("hmac_key")
    if not isinstance(key_b64, str) or not key_b64:
        msg = f"Missing hmac_key in {path}"
        raise ValueError(msg)

    key_id = payload.get("hmac_key_id")
    if not isinstance(key_id, str) or not key_id:
        key_id = "default"
    secret = base64.b64decode(key_b64)
    return key_id, secret


async def build_response_signature(
    body: bytes,
    root_path: Path | str,
    space_id: str,
) -> tuple[str, str]:
    """Compute the HMAC signature for the response body.

    Args:
        body: The response body bytes to sign.
        root_path: The root directory where spaces are stored.
        space_id: The target space identifier for key scoping.

    Returns:
        Tuple of (key_id, signature_hex).

    """
    key_id, secret = await anyio.to_thread.run_sync(
        _load_response_hmac_material,
        root_path,
        space_id,
    )
    signature = hmac.new(secret, body, hashlib.sha256).hexdigest()
    return key_id, signature
