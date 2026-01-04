"""Tests for notes management."""

import hashlib
import hmac
import json
from pathlib import Path
from typing import Any

import fsspec
import pytest

from ieapp.notes import RevisionMismatchError, create_note, list_notes, update_note
from ieapp.workspace import create_workspace

STRUCTURED_NOTE_CONTENT = """---
class: meeting
tags:
    - kickoff
---
# Kickoff

## Date
2025-11-29

## Summary
Wrap up
"""

HISTORY_LENGTH = 2


@pytest.fixture
def workspace_root(tmp_path: Path) -> Path:
    """Create a temporary workspace for testing."""
    root = tmp_path / "ieapp_root"
    ws_id = "test-workspace"
    create_workspace(root, ws_id)
    return root / "workspaces" / ws_id


@pytest.fixture
def fake_integrity_provider() -> Any:
    """Create a fake integrity provider for testing."""

    class _FakeIntegrityProvider:
        secret = b"unit-test-secret"

        def checksum(self, content: str) -> str:
            return hashlib.sha256(content.encode("utf-8")).hexdigest()

        def signature(self, content: str) -> str:
            return hmac.new(
                self.secret,
                content.encode("utf-8"),
                hashlib.sha256,
            ).hexdigest()

    return _FakeIntegrityProvider()


def test_create_note_basic(workspace_root: Path, fake_integrity_provider: Any) -> None:
    """Verifies that creating a note generates the required file structure."""
    note_id = "note-1"
    content = "# My Note\n\nHello World"

    create_note(
        workspace_root,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
    )

    note_path = workspace_root / "notes" / note_id
    assert note_path.exists()
    assert (note_path / "meta.json").exists()
    assert (note_path / "content.json").exists()
    assert (note_path / "history" / "index.json").exists()

    # Check content.json
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        assert data["markdown"] == content
        assert data["frontmatter"] == {}
        assert data["sections"] == {}
        assert "revision_id" in data
        assert "parent_revision_id" in data
        assert data["parent_revision_id"] is None
        assert "author" in data


def test_update_note_revision_mismatch(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note requires the correct parent_revision_id."""
    note_id = "note-2"
    content = "# Note 2"
    create_note(
        workspace_root,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
    )

    # Get current revision from content.json
    note_path = workspace_root / "notes" / note_id
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        current_rev = data["revision_id"]

    # Update with correct revision
    new_content = "# Note 2 Updated"
    update_note(
        workspace_root,
        note_id,
        new_content,
        parent_revision_id=current_rev,
        integrity_provider=fake_integrity_provider,
    )

    # Get new revision
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        new_rev = data["revision_id"]

    assert new_rev != current_rev

    # Try to update with OLD revision (should fail)
    with pytest.raises(RevisionMismatchError):
        update_note(
            workspace_root,
            note_id,
            "Should fail",
            parent_revision_id=current_rev,
            integrity_provider=fake_integrity_provider,
        )

    # Try to update with WRONG revision
    with pytest.raises(RevisionMismatchError):
        update_note(
            workspace_root,
            note_id,
            "Should fail",
            parent_revision_id="wrong-rev",
            integrity_provider=fake_integrity_provider,
        )


def test_note_history_append(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note appends to history and updates index."""
    note_id = "note-history"
    content_v1 = "# Version 1"
    create_note(
        workspace_root,
        note_id,
        content_v1,
        integrity_provider=fake_integrity_provider,
    )

    note_path = workspace_root / "notes" / note_id

    # Get v1 revision
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        rev_v1 = data["revision_id"]

    # Update to v2
    content_v2 = "# Version 2"
    update_note(
        workspace_root,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
        integrity_provider=fake_integrity_provider,
    )

    # Check history index
    with (note_path / "history" / "index.json").open() as f:
        history_index = json.load(f)
        assert history_index["note_id"] == note_id
        revisions = history_index["revisions"]
        assert len(revisions) == HISTORY_LENGTH
        assert revisions[0]["revision_id"] == rev_v1
        assert revisions[0]["checksum"] == fake_integrity_provider.checksum(content_v1)
        assert revisions[0]["signature"] == fake_integrity_provider.signature(
            content_v1,
        )

    # Check latest revision file
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        rev_v2 = data["revision_id"]

    with (note_path / "history" / f"{rev_v2}.json").open() as f:
        rev_data = json.load(f)
        assert rev_data["revision_id"] == rev_v2
        assert rev_data["parent_revision_id"] == rev_v1
        assert "diff" in rev_data
        assert rev_data["integrity"]["checksum"] == fake_integrity_provider.checksum(
            content_v2,
        )
        assert rev_data["integrity"]["signature"] == fake_integrity_provider.signature(
            content_v2,
        )


def test_markdown_sections_persist(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that frontmatter and sections persist to storage."""
    note_id = "note-structured"
    content = STRUCTURED_NOTE_CONTENT

    create_note(
        workspace_root,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
    )

    note_path = workspace_root / "notes" / note_id
    with (note_path / "content.json").open() as f:
        data = json.load(f)

    assert data["frontmatter"]["class"] == "meeting"
    assert data["sections"]["Date"] == "2025-11-29"
    assert data["sections"]["Summary"] == "Wrap up"


def test_note_history_diff(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note stores the diff in the history file."""
    note_id = "note-diff"
    content_v1 = "Line 1\nLine 2"
    create_note(
        workspace_root,
        note_id,
        content_v1,
        integrity_provider=fake_integrity_provider,
    )

    note_path = workspace_root / "notes" / note_id
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        rev_v1 = data["revision_id"]

    content_v2 = "Line 1\nLine 2 Modified"
    update_note(
        workspace_root,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
        integrity_provider=fake_integrity_provider,
    )

    with (note_path / "content.json").open() as f:
        data = json.load(f)
        rev_v2 = data["revision_id"]

    with (note_path / "history" / f"{rev_v2}.json").open() as f:
        rev_data = json.load(f)
        assert "diff" in rev_data
        assert "Line 2 Modified" in rev_data["diff"]


def test_note_lifecycle_with_memory_fs(fake_integrity_provider: Any) -> None:
    """Ensure note CRUD works when using a non-file fsspec filesystem."""
    fs = fsspec.filesystem("memory", skip_instance_cache=True)
    root = "/ieapp"
    ws_id = "mem-ws"

    create_workspace(root, ws_id, fs=fs)
    ws_path = f"{root}/workspaces/{ws_id}"

    create_note(
        ws_path,
        "note-1",
        "# Title\n\ncontent",
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    with fs.open(f"{ws_path}/notes/note-1/content.json", "r") as handle:
        content_json = json.load(handle)
    initial_rev = content_json["revision_id"]

    update_note(
        ws_path,
        "note-1",
        "# Title\n\nupdated",
        parent_revision_id=initial_rev,
        integrity_provider=fake_integrity_provider,
        fs=fs,
        attachments=[{"id": "a1", "name": "file", "path": "attachments/a1"}],
    )

    with fs.open(f"{ws_path}/notes/note-1/content.json", "r") as handle:
        updated_json = json.load(handle)

    assert updated_json["revision_id"] != initial_rev
    assert updated_json.get("attachments") == [
        {"id": "a1", "name": "file", "path": "attachments/a1"},
    ]


def test_note_author_persistence(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that the author field is persisted correctly."""
    note_id = "note-author"
    content = "# Author Test"
    author = "agent-smith"

    create_note(
        workspace_root,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        author=author,
    )

    note_path = workspace_root / "notes" / note_id
    with (note_path / "content.json").open() as f:
        data = json.load(f)
        assert data["author"] == author
        rev_id = data["revision_id"]

    with (note_path / "history" / f"{rev_id}.json").open() as f:
        data = json.load(f)
        assert data["author"] == author

    # Update with different author
    new_author = "neo"
    update_note(
        workspace_root,
        note_id,
        "# Author Test Updated",
        parent_revision_id=rev_id,
        integrity_provider=fake_integrity_provider,
        author=new_author,
    )

    with (note_path / "content.json").open() as f:
        data = json.load(f)
        assert data["author"] == new_author
        rev_id_2 = data["revision_id"]

    with (note_path / "history" / f"{rev_id_2}.json").open() as f:
        data = json.load(f)
        assert data["author"] == new_author


def test_list_notes_returns_properties_and_links(
    workspace_root: Path,
    fake_integrity_provider: Any,
) -> None:
    """Verifies that list_notes returns properties and links fields."""
    note_id = "note-with-props"
    content = "# Test Note\n\nSome content"

    create_note(
        workspace_root,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
    )

    notes = list_notes(workspace_root)
    assert len(notes) == 1

    note = notes[0]
    assert note["id"] == note_id
    assert "properties" in note, (
        "properties field must be present in list_notes response"
    )
    assert "links" in note, "links field must be present in list_notes response"
    assert isinstance(note["properties"], dict)
    assert isinstance(note["links"], list)
