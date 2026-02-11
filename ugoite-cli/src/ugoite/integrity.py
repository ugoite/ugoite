"""Integrity helpers for entry storage.

This module centralizes checksum and signature logic so tests can mock the
provider while the production code derives its secret from ``global.json``.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from pathlib import Path

    import fsspec

from .utils import fs_exists, fs_join, fs_read_json, get_fs_and_path


@dataclass
class IntegrityProvider:
    """Computes checksums and signatures for entry revisions."""

    secret: bytes

    @classmethod
    def for_space(
        cls,
        space_path: str | Path,
        *,
        fs: fsspec.AbstractFileSystem | None = None,
    ) -> IntegrityProvider:
        """Build a provider using the space's root ``global.json``.

        Args:
            space_path: Absolute path to the space directory.
            fs: Optional fsspec filesystem to use.

        Returns:
            An ``IntegrityProvider`` configured with the space's HMAC key.

        Raises:
            FileNotFoundError: If ``global.json`` cannot be located.
            ValueError: If the file does not contain the expected key material.

        """
        fs_obj, ws_path = get_fs_and_path(space_path, fs)
        meta_path = fs_join(ws_path, "meta.json")

        if not fs_exists(fs_obj, meta_path):
            msg = f"meta.json not found for space at {space_path}"
            raise FileNotFoundError(msg)

        meta: dict[str, Any] = fs_read_json(fs_obj, meta_path)

        storage_root = meta.get("storage", {}).get("root")
        if not storage_root:
            msg = "Space metadata missing storage.root"
            raise ValueError(msg)

        storage_fs, storage_path = get_fs_and_path(storage_root, fs_obj)
        global_json = fs_join(storage_path, "global.json")

        if not fs_exists(storage_fs, global_json):
            msg = f"global.json not found for space at {storage_root}"
            raise FileNotFoundError(msg)

        global_data = fs_read_json(storage_fs, global_json)

        key_b64 = global_data.get("hmac_key")
        if not key_b64:
            msg = "Missing hmac_key in global.json"
            raise ValueError(msg)

        try:
            secret = base64.b64decode(key_b64)
        except Exception as exc:  # pragma: no cover - defensive
            msg = "Failed to decode hmac_key"
            raise ValueError(msg) from exc

        return cls(secret=secret)

    def checksum(self, payload: str) -> str:
        """Compute the SHA-256 checksum.

        Args:
            payload: Raw string to hash.

        Returns:
            Hex-encoded SHA-256 digest.

        """
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()

    def signature(self, payload: str) -> str:
        """Compute the HMAC signature for ``payload``.

        Args:
            payload: Raw string to sign.

        Returns:
            Hex-encoded HMAC digest derived from the provider secret.

        """
        return hmac.new(
            self.secret,
            payload.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
