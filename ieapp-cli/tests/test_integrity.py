"""Tests for integrity provider.

REQ-INT-001: HMAC signature.
"""

import json
import re
from pathlib import Path

import pytest

from ieapp.integrity import IntegrityProvider
from ieapp.space import create_space

HMAC_KEY_LENGTH = 32


def test_integrity_provider_for_space_success(tmp_path: Path) -> None:
    """Verifies that IntegrityProvider loads correctly from a valid space."""
    root = tmp_path / "ieapp_root"
    ws_id = "test-space"
    create_space(root, ws_id)
    ws_path = root / "spaces" / ws_id

    provider = IntegrityProvider.for_space(ws_path)
    assert isinstance(provider, IntegrityProvider)
    assert len(provider.secret) == HMAC_KEY_LENGTH


def test_integrity_provider_missing_meta(tmp_path: Path) -> None:
    """Verifies error when meta.json is missing."""
    ws_path = tmp_path / "fake-space"
    ws_path.mkdir()

    with pytest.raises(FileNotFoundError, match=re.escape("meta.json not found")):
        IntegrityProvider.for_space(ws_path)


def test_integrity_provider_missing_storage_root(tmp_path: Path) -> None:
    """Verifies error when storage.root is missing in meta.json."""
    ws_path = tmp_path / "fake-space"
    ws_path.mkdir()

    meta_path = ws_path / "meta.json"
    with meta_path.open("w") as f:
        json.dump({"id": "fake"}, f)

    with pytest.raises(
        ValueError,
        match=re.escape("Space metadata missing storage.root"),
    ):
        IntegrityProvider.for_space(ws_path)


def test_integrity_provider_missing_global_json(tmp_path: Path) -> None:
    """Verifies error when global.json is missing at storage root."""
    ws_path = tmp_path / "fake-space"
    ws_path.mkdir()

    # Point to a non-existent root
    root_path = tmp_path / "non-existent-root"

    meta_path = ws_path / "meta.json"
    with meta_path.open("w") as f:
        json.dump({"storage": {"root": str(root_path)}}, f)

    with pytest.raises(FileNotFoundError, match=re.escape("global.json not found")):
        IntegrityProvider.for_space(ws_path)


def test_integrity_provider_missing_hmac_key(tmp_path: Path) -> None:
    """Verifies error when hmac_key is missing in global.json."""
    root = tmp_path / "ieapp_root"
    root.mkdir()
    ws_path = root / "spaces" / "test-space"
    ws_path.mkdir(parents=True)

    meta_path = ws_path / "meta.json"
    with meta_path.open("w") as f:
        json.dump({"storage": {"root": str(root)}}, f)

    global_json = root / "global.json"
    with global_json.open("w") as f:
        json.dump({"version": 1}, f)

    with pytest.raises(ValueError, match="Missing hmac_key"):
        IntegrityProvider.for_space(ws_path)


def test_integrity_provider_invalid_hmac_key(tmp_path: Path) -> None:
    """Verifies error when hmac_key is not valid base64."""
    root = tmp_path / "ieapp_root"
    root.mkdir()
    ws_path = root / "spaces" / "test-space"
    ws_path.mkdir(parents=True)

    meta_path = ws_path / "meta.json"
    with meta_path.open("w") as f:
        json.dump({"storage": {"root": str(root)}}, f)

    global_json = root / "global.json"
    with global_json.open("w") as f:
        json.dump({"hmac_key": "not-base64!"}, f)

    with pytest.raises(ValueError, match="Failed to decode hmac_key"):
        IntegrityProvider.for_space(ws_path)
