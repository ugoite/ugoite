import pytest
import json
import os
from ieapp.workspace import create_workspace
from ieapp.notes import create_note, update_note, NoteExistsError, RevisionMismatchError
import hashlib


@pytest.fixture
def workspace_root(tmp_path):
    root = tmp_path / "ieapp_root"
    ws_id = "test-workspace"
    create_workspace(root, ws_id)
    return root / "workspaces" / ws_id


def test_create_note_basic(workspace_root):
    """
    Verifies that creating a note generates the required file structure.
    """
    note_id = "note-1"
    content = "# My Note\n\nHello World"

    create_note(workspace_root, note_id, content)

    note_path = workspace_root / "notes" / note_id
    assert note_path.exists()
    assert (note_path / "meta.json").exists()
    assert (note_path / "content.json").exists()
    assert (note_path / "history" / "index.json").exists()

    # Check content.json
    with open(note_path / "content.json") as f:
        data = json.load(f)
        assert data["markdown"] == content
        assert "frontmatter" in data
        assert "revision_id" in data
        assert "author" in data


def test_update_note_revision_mismatch(workspace_root):
    """
    Verifies that updating a note requires the correct parent_revision_id.
    """
    note_id = "note-2"
    content = "# Note 2"
    create_note(workspace_root, note_id, content)

    # Get current revision from content.json
    note_path = workspace_root / "notes" / note_id
    with open(note_path / "content.json") as f:
        data = json.load(f)
        current_rev = data["revision_id"]

    # Update with correct revision
    new_content = "# Note 2 Updated"
    update_note(workspace_root, note_id, new_content, parent_revision_id=current_rev)

    # Get new revision
    with open(note_path / "content.json") as f:
        data = json.load(f)
        new_rev = data["revision_id"]

    assert new_rev != current_rev

    # Try to update with OLD revision (should fail)
    with pytest.raises(RevisionMismatchError):
        update_note(
            workspace_root, note_id, "Should fail", parent_revision_id=current_rev
        )

    # Try to update with WRONG revision
    with pytest.raises(RevisionMismatchError):
        update_note(
            workspace_root, note_id, "Should fail", parent_revision_id="wrong-rev"
        )


def test_note_history_append(workspace_root):
    """
    Verifies that updating a note appends to history and updates index.
    """
    note_id = "note-history"
    content_v1 = "# Version 1"
    create_note(workspace_root, note_id, content_v1)

    note_path = workspace_root / "notes" / note_id

    # Get v1 revision
    with open(note_path / "content.json") as f:
        data = json.load(f)
        rev_v1 = data["revision_id"]

    # Update to v2
    content_v2 = "# Version 2"
    update_note(workspace_root, note_id, content_v2, parent_revision_id=rev_v1)

    # Check history index
    with open(note_path / "history" / "index.json") as f:
        history_index = json.load(f)
        assert history_index["note_id"] == note_id
        revisions = history_index["revisions"]
        assert len(revisions) == 2
        assert revisions[0]["revision_id"] == rev_v1

    # Check latest revision file
    with open(note_path / "content.json") as f:
        data = json.load(f)
        rev_v2 = data["revision_id"]

    with open(note_path / "history" / f"{rev_v2}.json") as f:
        rev_data = json.load(f)
        assert rev_data["revision_id"] == rev_v2
        assert rev_data["parent_revision_id"] == rev_v1
        assert (
            rev_data["integrity"]["checksum"]
            == hashlib.sha256(content_v2.encode("utf-8")).hexdigest()
        )
