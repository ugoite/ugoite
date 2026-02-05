"""Tests for space management.

REQ-STO-001: Storage abstraction using fsspec.
REQ-STO-002: Space directory structure.
REQ-STO-003: File permissions.
REQ-STO-004: Space management via global.json.
REQ-STO-005: Prevent duplicate space creation.
"""

import json

import fsspec
import pytest

from ieapp.space import SpaceExistsError, create_space
from ieapp.utils import fs_join


def test_create_space_scaffolding(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that create_space generates the required file structure.

    It also verifies metadata files as per Spec 03 §2.
    """
    fs, root = fs_impl
    ws_id = "test-space"

    # Call the function to be implemented
    create_space(root, ws_id, fs=fs)

    # Verify directory structure
    ws_path = fs_join(root, "spaces", ws_id)
    assert fs.exists(ws_path)
    assert fs.exists(fs_join(ws_path, "meta.json"))
    assert fs.exists(fs_join(ws_path, "settings.json"))
    assert fs.exists(fs_join(ws_path, "forms"))
    assert fs.exists(fs_join(ws_path, "assets"))

    # Verify global.json exists at root
    global_json_path = fs_join(root, "global.json")
    assert fs.exists(global_json_path)

    # Verify global.json content includes the new space
    with fs.open(global_json_path, "r") as f:
        global_data = json.load(f)
        assert "spaces" in global_data
        assert ws_id in global_data["spaces"]

    # Verify meta.json content
    with fs.open(fs_join(ws_path, "meta.json"), "r") as f:
        meta = json.load(f)
        assert meta["id"] == ws_id
        assert meta["name"] == ws_id
        assert "created_at" in meta
        assert "storage" in meta

    # Permission enforcement is handled by the core storage layer.


def test_create_space_idempotency(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that creating an existing space raises SpaceExistsError."""
    fs, root = fs_impl
    ws_id = "test-space"
    create_space(root, ws_id, fs=fs)

    # Should raise error if exists
    with pytest.raises(SpaceExistsError):
        create_space(root, ws_id, fs=fs)


def test_create_space_s3_unimplemented() -> None:
    """Verifies that providing a non-local URI (e.g. s3://) raises NotImplementedError.

    This is for Milestone 0.
    """
    root = "s3://my-bucket/ieapp_root"
    ws_id = "test-space"

    # We expect this to fail because we only support local fs in M0
    # or we haven't configured S3 credentials/mocking.
    # But the requirement is to "raise unimplemented error".
    with pytest.raises(NotImplementedError):
        create_space(root, ws_id)
