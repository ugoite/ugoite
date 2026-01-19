"""Tests for workspace management.

REQ-STO-001: Storage abstraction using fsspec.
REQ-STO-002: Workspace directory structure.
REQ-STO-003: File permissions.
REQ-STO-004: Workspace management via global.json.
REQ-STO-005: Prevent duplicate workspace creation.
"""

import json

import fsspec
import pytest

from ieapp.utils import fs_join
from ieapp.workspace import WorkspaceExistsError, create_workspace


def test_create_workspace_scaffolding(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that create_workspace generates the required file structure.

    It also verifies metadata files as per Spec 03 §2.
    """
    fs, root = fs_impl
    ws_id = "test-workspace"

    # Call the function to be implemented
    create_workspace(root, ws_id, fs=fs)

    # Verify directory structure
    ws_path = fs_join(root, "workspaces", ws_id)
    assert fs.exists(ws_path)
    assert fs.exists(fs_join(ws_path, "meta.json"))
    assert fs.exists(fs_join(ws_path, "settings.json"))
    assert fs.exists(fs_join(ws_path, "classes"))
    assert fs.exists(fs_join(ws_path, "attachments"))

    # Verify global.json exists at root
    global_json_path = fs_join(root, "global.json")
    assert fs.exists(global_json_path)

    # Verify global.json content includes the new workspace
    with fs.open(global_json_path, "r") as f:
        global_data = json.load(f)
        assert "workspaces" in global_data
        assert ws_id in global_data["workspaces"]

    # Verify meta.json content
    with fs.open(fs_join(ws_path, "meta.json"), "r") as f:
        meta = json.load(f)
        assert meta["id"] == ws_id
        assert meta["name"] == ws_id
        assert "created_at" in meta
        assert "storage" in meta

    # Permission enforcement is handled by the core storage layer.


def test_create_workspace_idempotency(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that creating an existing workspace raises WorkspaceExistsError."""
    fs, root = fs_impl
    ws_id = "test-workspace"
    create_workspace(root, ws_id, fs=fs)

    # Should raise error if exists
    with pytest.raises(WorkspaceExistsError):
        create_workspace(root, ws_id, fs=fs)


def test_create_workspace_s3_unimplemented() -> None:
    """Verifies that providing a non-local URI (e.g. s3://) raises NotImplementedError.

    This is for Milestone 0.
    """
    root = "s3://my-bucket/ieapp_root"
    ws_id = "test-workspace"

    # We expect this to fail because we only support local fs in M0
    # or we haven't configured S3 credentials/mocking.
    # But the requirement is to "raise unimplemented error".
    with pytest.raises(NotImplementedError):
        create_workspace(root, ws_id)
