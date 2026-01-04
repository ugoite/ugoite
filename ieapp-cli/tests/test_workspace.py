"""Tests for workspace management."""

import json
from pathlib import Path

import fsspec
import pytest

from ieapp.workspace import WorkspaceExistsError, create_workspace


def test_create_workspace_scaffolding(tmp_path: Path) -> None:
    """Verifies that create_workspace generates the required file structure.

    It also verifies metadata files as per Spec 03 §2.
    """
    root = tmp_path / "ieapp_root"
    ws_id = "test-workspace"

    # Call the function to be implemented
    create_workspace(root, ws_id)

    # Verify directory structure
    ws_path = root / "workspaces" / ws_id
    assert ws_path.exists()
    assert (ws_path / "meta.json").exists()
    assert (ws_path / "settings.json").exists()
    assert (ws_path / "schemas").exists()
    assert (ws_path / "index").exists()
    assert (ws_path / "attachments").exists()
    assert (ws_path / "notes").exists()
    assert (ws_path / "index" / "index.json").exists()
    assert (ws_path / "index" / "stats.json").exists()

    # Verify global.json exists at root
    assert (root / "global.json").exists()

    # Verify global.json content includes the new workspace
    with (root / "global.json").open() as f:
        global_data = json.load(f)
        assert "workspaces" in global_data
        assert ws_id in global_data["workspaces"]

    # Verify meta.json content
    with (ws_path / "meta.json").open() as f:
        meta = json.load(f)
        assert meta["id"] == ws_id
        assert meta["name"] == ws_id
        assert "created_at" in meta
        assert "storage" in meta

    # Verify permissions
    # (chmod 600 for files/dirs - strictly 700 for dirs, 600 for files)
    # Spec 05 §1: "The app enforces chmod 600 on the data directory."
    # Usually dirs need +x (700) to be traversable by owner.
    # Let's check if it's private.
    mode = ws_path.stat().st_mode
    # Check if group/other have no permissions
    assert (mode & 0o077) == 0


def test_create_workspace_idempotency(tmp_path: Path) -> None:
    """Verifies that creating an existing workspace raises WorkspaceExistsError."""
    root = tmp_path / "ieapp_root"
    ws_id = "test-workspace"
    create_workspace(root, ws_id)

    # Should raise error if exists? Or be idempotent?
    # Task says "happy path + 409 conflict" for API.
    # Library should probably raise exception.
    with pytest.raises(WorkspaceExistsError):
        create_workspace(root, ws_id)


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


def test_create_workspace_with_memory_fs() -> None:
    """Create workspace using an in-memory fsspec filesystem."""
    fs = fsspec.filesystem("memory", skip_instance_cache=True)
    root = "/ieapp"
    ws_id = "mem-ws"

    create_workspace(root, ws_id, fs=fs)

    ws_path = f"{root}/workspaces/{ws_id}"
    assert fs.exists(ws_path)
    assert fs.exists(f"{ws_path}/meta.json")
    assert fs.exists(f"{ws_path}/settings.json")
    assert fs.exists(f"{ws_path}/schemas")
    assert fs.exists(f"{ws_path}/index/index.json")
    assert fs.exists(f"{ws_path}/index/stats.json")
    assert fs.exists(f"{ws_path}/attachments")
    assert fs.exists(f"{ws_path}/notes")

    with fs.open(f"{root}/global.json", "r") as handle:
        global_data = json.load(handle)
    assert ws_id in global_data.get("workspaces", [])
