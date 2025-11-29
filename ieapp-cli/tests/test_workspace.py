import pytest
import json
import os
from pathlib import Path
from ieapp.workspace import create_workspace, WorkspaceExistsError


def test_create_workspace_scaffolding(tmp_path):
    """
    Verifies that create_workspace generates the required file structure
    and metadata files as per Spec 03 §2.
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

    # Verify global.json exists at root
    assert (root / "global.json").exists()

    # Verify global.json content includes the new workspace
    with open(root / "global.json") as f:
        global_data = json.load(f)
        assert "workspaces" in global_data
        # Check if ws_id is in the list of workspaces
        # Assuming workspaces is a list of IDs or objects with ID
        # Based on spec 03 §2.1: "Holds workspace registry"
        # Let's assume it's a list of objects for now, or just IDs.
        # The implementation initialized it as {"workspaces": []}
        # So we expect the new workspace to be added there.

        # We need to check if the implementation adds it.
        # If it's a list of strings:
        # assert ws_id in global_data["workspaces"]
        # If it's a list of objects:
        # assert any(ws["id"] == ws_id for ws in global_data["workspaces"])

        # Let's assume list of IDs for simplicity or check what implementation does.
        # Implementation currently does NOT update it.
        # So I will write the test to expect it to be there.
        assert ws_id in global_data["workspaces"]

    # Verify meta.json content
    with open(ws_path / "meta.json") as f:
        meta = json.load(f)
        assert meta["id"] == ws_id
        assert "created_at" in meta
        assert "storage" in meta

    # Verify permissions (chmod 600 for files/dirs - strictly 700 for dirs, 600 for files)
    # Spec 05 §1: "The app enforces chmod 600 on the data directory."
    # Usually dirs need +x (700) to be traversable by owner.
    # Let's check if it's private.
    mode = ws_path.stat().st_mode
    # Check if group/other have no permissions
    assert (mode & 0o077) == 0


def test_create_workspace_idempotency(tmp_path):
    root = tmp_path / "ieapp_root"
    ws_id = "test-workspace"
    create_workspace(root, ws_id)

    # Should raise error if exists? Or be idempotent?
    # Task says "happy path + 409 conflict" for API.
    # Library should probably raise exception.
    with pytest.raises(WorkspaceExistsError):
        create_workspace(root, ws_id)


def test_create_workspace_s3_unimplemented():
    """
    Verifies that providing a non-local URI (e.g. s3://) raises NotImplementedError
    or similar for Milestone 0.
    """
    root = "s3://my-bucket/ieapp_root"
    ws_id = "test-workspace"

    # We expect this to fail because we only support local fs in M0
    # or we haven't configured S3 credentials/mocking.
    # But the requirement is to "raise unimplemented error".
    with pytest.raises(NotImplementedError):
        create_workspace(root, ws_id)
