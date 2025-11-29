"""Tests for workspace management."""

import json
from pathlib import Path

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
    assert ws_path.exists()  # noqa: S101
    assert (ws_path / "meta.json").exists()  # noqa: S101
    assert (ws_path / "settings.json").exists()  # noqa: S101
    assert (ws_path / "schemas").exists()  # noqa: S101
    assert (ws_path / "index").exists()  # noqa: S101
    assert (ws_path / "attachments").exists()  # noqa: S101
    assert (ws_path / "notes").exists()  # noqa: S101
    assert (ws_path / "index" / "index.json").exists()  # noqa: S101
    assert (ws_path / "index" / "stats.json").exists()  # noqa: S101

    # Verify global.json exists at root
    assert (root / "global.json").exists()  # noqa: S101

    # Verify global.json content includes the new workspace
    with (root / "global.json").open() as f:
        global_data = json.load(f)
        assert "workspaces" in global_data  # noqa: S101
        assert ws_id in global_data["workspaces"]  # noqa: S101

    # Verify meta.json content
    with (ws_path / "meta.json").open() as f:
        meta = json.load(f)
        assert meta["id"] == ws_id  # noqa: S101
        assert meta["name"] == ws_id  # noqa: S101
        assert "created_at" in meta  # noqa: S101
        assert "storage" in meta  # noqa: S101

    # Verify permissions
    # (chmod 600 for files/dirs - strictly 700 for dirs, 600 for files)
    # Spec 05 §1: "The app enforces chmod 600 on the data directory."
    # Usually dirs need +x (700) to be traversable by owner.
    # Let's check if it's private.
    mode = ws_path.stat().st_mode
    # Check if group/other have no permissions
    assert (mode & 0o077) == 0  # noqa: S101


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
