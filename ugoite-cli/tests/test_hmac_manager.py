"""Tests for HMAC manager module with multiple fsspec implementations.

REQ-INT-001: HMAC signature management.
"""

import json
from pathlib import Path

import fsspec
import pytest

from ugoite.hmac_manager import (
    build_response_signature,
    ensure_global_json,
    load_hmac_material,
)
from ugoite.utils import fs_exists, fs_join, fs_read_json, get_fs_and_path


def test_load_hmac_material_creates_global_json_local(tmp_path: Path) -> None:
    """Test that load_hmac_material creates global.json on local filesystem."""
    root = tmp_path / "ugoite_root"

    # Call load_hmac_material - should create global.json
    key_id, secret = load_hmac_material(root)

    # Verify global.json was created
    global_json = root / "global.json"
    assert global_json.exists()

    # Verify returned values
    assert isinstance(key_id, str)
    assert key_id.startswith("key-")
    assert isinstance(secret, bytes)
    assert len(secret) == 32  # 32 bytes for HMAC-SHA256

    # Verify file permissions (Unix-only)
    if hasattr(global_json, "stat"):
        mode = global_json.stat().st_mode & 0o777
        assert mode == 0o600


def test_load_hmac_material_uses_existing_global_json_local(tmp_path: Path) -> None:
    """Test that load_hmac_material uses existing global.json on local filesystem."""
    root = tmp_path / "ugoite_root"

    # Create global.json first time
    key_id_1, secret_1 = load_hmac_material(root)

    # Load again - should use existing
    key_id_2, secret_2 = load_hmac_material(root)

    # Should return same values
    assert key_id_1 == key_id_2
    assert secret_1 == secret_2


def test_build_response_signature_local(tmp_path: Path) -> None:
    """Test build_response_signature on local filesystem."""
    root = tmp_path / "ugoite_root"
    test_body = b"test response body"

    # Build signature
    key_id, signature = build_response_signature(test_body, root)

    # Verify signature format
    assert isinstance(key_id, str)
    assert isinstance(signature, str)
    assert len(signature) == 64  # SHA256 hex = 64 chars
    assert all(c in "0123456789abcdef" for c in signature)

    # Build again - should be same signature
    key_id_2, signature_2 = build_response_signature(test_body, root)
    assert key_id == key_id_2
    assert signature == signature_2

    # Different body should give different signature
    different_body = b"different body"
    _, signature_3 = build_response_signature(different_body, root)
    assert signature != signature_3


def test_ensure_global_json_local(tmp_path: Path) -> None:
    """Test ensure_global_json on local filesystem."""
    root = tmp_path / "ugoite_root"

    # Ensure global.json is created
    ensure_global_json(root)

    # Verify it exists
    global_json = root / "global.json"
    assert global_json.exists()

    # Verify content structure
    with global_json.open() as f:
        data = json.load(f)

    assert "version" in data
    assert "hmac_key" in data
    assert "hmac_key_id" in data
    assert "spaces" in data
    assert data["version"] == 1


def test_ensure_global_json_memory_fs() -> None:
    """Test ensure_global_json with memory:// filesystem."""
    fs = fsspec.filesystem("memory")
    root_path = "/ensure-test"

    # Ensure global.json is created
    ensure_global_json(root_path, fs=fs)

    # Verify it exists in memory
    fs_obj, base_path = get_fs_and_path(root_path, fs)
    global_json_path = fs_join(base_path, "global.json")
    assert fs_exists(fs_obj, global_json_path)

    # Verify content
    global_data = fs_read_json(fs_obj, global_json_path)
    assert "version" in global_data
    assert "hmac_key" in global_data
    assert global_data["version"] == 1


def test_ensure_global_json_idempotent_local(tmp_path: Path) -> None:
    """Test that ensure_global_json is idempotent on local filesystem."""
    root = tmp_path / "ugoite_root"

    # Create first time
    ensure_global_json(root)
    global_json = root / "global.json"

    with global_json.open() as f:
        data_1 = json.load(f)

    # Call again - should not overwrite
    ensure_global_json(root)

    with global_json.open() as f:
        data_2 = json.load(f)

    # Should be identical
    assert data_1 == data_2


def test_ensure_global_json_idempotent_memory_fs() -> None:
    """Test that ensure_global_json is idempotent with memory filesystem."""
    fs = fsspec.filesystem("memory")
    root_path = "/idempotent-test"

    # Create first time
    ensure_global_json(root_path, fs=fs)

    fs_obj, base_path = get_fs_and_path(root_path, fs)
    global_json_path = fs_join(base_path, "global.json")
    data_1 = fs_read_json(fs_obj, global_json_path)

    # Call again - should not overwrite
    ensure_global_json(root_path, fs=fs)
    data_2 = fs_read_json(fs_obj, global_json_path)

    # Should be identical
    assert data_1 == data_2


def test_load_hmac_material_missing_key_error(tmp_path: Path) -> None:
    """Test that load_hmac_material raises error if hmac_key is missing."""
    root = tmp_path / "ugoite_root"
    root.mkdir()

    # Create invalid global.json without hmac_key
    global_json = root / "global.json"
    with global_json.open("w") as f:
        json.dump({"version": 1, "hmac_key_id": "test"}, f)

    # Should raise ValueError
    with pytest.raises(ValueError, match="Missing hmac_key"):
        load_hmac_material(root)
