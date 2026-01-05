"""HMAC key management using fsspec for data integrity."""

from __future__ import annotations

import base64
import hashlib
import hmac
import logging
import secrets
import uuid
from datetime import UTC, datetime
from functools import lru_cache
from typing import TYPE_CHECKING

from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
)

if TYPE_CHECKING:
    from pathlib import Path

    import fsspec

logger = logging.getLogger(__name__)


@lru_cache(maxsize=32)
def _load_hmac_material_cached(
    root_path: str,
    protocol: str,
) -> tuple[str, bytes]:
    """Cache wrapper for HMAC material loading.

    Args:
        root_path: The root directory path
        protocol: The filesystem protocol (file, s3, memory, etc.) - used as cache key

    Returns:
        Tuple of (key_id, secret_bytes)

    """
    # The actual loading is done by the non-cached function
    # This is just a cache key wrapper
    return _load_hmac_material_impl(root_path)


def _load_hmac_material_impl(
    root_path: str,
) -> tuple[str, bytes]:
    """Load HMAC key material from global.json using fsspec.

    Args:
        root_path: The root directory path

    Returns:
        Tuple of (key_id, secret_bytes)

    Raises:
        ValueError: If hmac_key is missing from global.json

    """
    fs_obj, base_path = get_fs_and_path(root_path)
    global_json_path = fs_join(base_path, "global.json")

    if not fs_exists(fs_obj, global_json_path):
        _write_default_global(fs_obj, base_path)

    global_data = fs_read_json(fs_obj, global_json_path)

    key_b64 = global_data.get("hmac_key")
    key_id = global_data.get("hmac_key_id", "default")

    if not key_b64:
        msg = "Missing hmac_key in global.json"
        raise ValueError(msg)

    secret = base64.b64decode(key_b64)
    return key_id, secret


def _write_default_global(
    fs: fsspec.AbstractFileSystem,  # type: ignore[name-defined]
    root_path: str,
) -> None:
    """Write a default global.json with a random HMAC key using fsspec.

    Args:
        fs: The filesystem object
        root_path: The root directory path

    """
    global_json_path = fs_join(root_path, "global.json")

    # Check if it already exists (race condition prevention)
    if fs_exists(fs, global_json_path):
        return

    # Ensure the root directory exists
    fs_makedirs(fs, root_path, exist_ok=True)

    protocol = getattr(fs, "protocol", "file") or "file"
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    now_iso = datetime.now(UTC).isoformat()
    key_id = f"key-{uuid.uuid4().hex}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"{protocol}://{root_path}",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    fs_write_json(fs, global_json_path, payload, mode=0o600, exclusive=True)
    logger.info("Created global.json at %s", global_json_path)


def load_hmac_material(root_path: str | Path) -> tuple[str, bytes]:
    """Load (or create) the global HMAC key for root_path using fsspec.

    This function ensures that global.json exists and returns the HMAC
    key material. It uses fsspec exclusively for all file operations.

    Args:
        root_path: The root directory where global.json is stored

    Returns:
        Tuple of (key_id, secret_bytes)

    Raises:
        ValueError: If hmac_key is missing from global.json

    """
    root_str = str(root_path)
    fs_obj, base_path = get_fs_and_path(root_str)

    protocol = getattr(fs_obj, "protocol", "file") or "file"
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    return _load_hmac_material_cached(base_path, protocol)


def build_response_signature(
    body: bytes,
    root_path: str | Path,
) -> tuple[str, str]:
    """Compute the HMAC signature for the response body.

    Args:
        body: The response body bytes to sign
        root_path: The root directory where global.json is stored

    Returns:
        Tuple of (key_id, signature_hex)

    """
    key_id, secret = load_hmac_material(root_path)
    signature = hmac.new(secret, body, hashlib.sha256).hexdigest()
    return key_id, signature


def ensure_global_json(
    root_path: str | Path,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Ensure global.json exists at root_path using fsspec.

    This is a helper function that can be called to initialize the
    global.json file if it doesn't exist.

    Args:
        root_path: The root directory where global.json should exist
        fs: Optional filesystem object to use (for testing)

    """
    fs_obj, base_path = get_fs_and_path(root_path, fs)
    global_json_path = fs_join(base_path, "global.json")

    if not fs_exists(fs_obj, global_json_path):
        _write_default_global(fs_obj, base_path)
