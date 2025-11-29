"""Integrity helpers for note storage.

This module centralizes checksum and signature logic so tests can mock the
provider while the production code derives its secret from ``global.json``.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Union


@dataclass
class IntegrityProvider:
    """Computes checksums and signatures for note revisions."""

    secret: bytes

    @classmethod
    def for_workspace(cls, workspace_path: Union[str, Path]) -> "IntegrityProvider":
        """Builds a provider using the workspace's root ``global.json``.

        Args:
            workspace_path: Absolute path to the workspace directory.

        Returns:
            An ``IntegrityProvider`` configured with the workspace's HMAC key.

        Raises:
            FileNotFoundError: If ``global.json`` cannot be located.
            ValueError: If the file does not contain the expected key material.
        """

        ws_path = Path(workspace_path)
        meta_path = ws_path / "meta.json"

        if not meta_path.exists():
            raise FileNotFoundError(
                f"meta.json not found for workspace at {workspace_path}"
            )

        with meta_path.open("r", encoding="utf-8") as meta_handle:
            meta = json.load(meta_handle)

        storage_root = meta.get("storage", {}).get("root")
        if not storage_root:
            raise ValueError("Workspace metadata missing storage.root")

        global_json = Path(storage_root) / "global.json"

        if not global_json.exists():
            raise FileNotFoundError(
                f"global.json not found for workspace at {storage_root}"
            )

        with global_json.open("r", encoding="utf-8") as handle:
            global_data = json.load(handle)

        key_b64 = global_data.get("hmac_key")
        if not key_b64:
            raise ValueError("Missing hmac_key in global.json")

        try:
            secret = base64.b64decode(key_b64)
        except Exception as exc:  # pragma: no cover - defensive
            raise ValueError("Failed to decode hmac_key") from exc

        return cls(secret=secret)

    def checksum(self, payload: str) -> str:
        """Computes the SHA-256 checksum.

        Args:
            payload: Raw string to hash.

        Returns:
            Hex-encoded SHA-256 digest.
        """

        return hashlib.sha256(payload.encode("utf-8")).hexdigest()

    def signature(self, payload: str) -> str:
        """Computes the HMAC signature for ``payload``.

        Args:
            payload: Raw string to sign.

        Returns:
            Hex-encoded HMAC digest derived from the provider secret.
        """

        return hmac.new(
            self.secret, payload.encode("utf-8"), hashlib.sha256
        ).hexdigest()
