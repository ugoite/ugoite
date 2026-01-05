"""Tests for notes."""

import fsspec

"""Tests for notes management."""

import hashlib
import hmac
import json
from typing import Any

import pytest

from ieapp.notes import RevisionMismatchError, create_note, list_notes, update_note
from ieapp.utils import fs_join
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
def workspace_root(fs_impl: tuple[fsspec.AbstractFileSystem, str]) -> tuple[Any, str]:
    """Create a temporary workspace for testing."""
    fs, root = fs_impl
    ws_id = "test-workspace"
    create_workspace(root, ws_id, fs=fs)
    ws_path = fs_join(root, "workspaces", ws_id)
    return fs, ws_path


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


def test_create_note_basic(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that creating a note generates the required file structure."""
    fs, ws_path = workspace_root
    note_id = "note-1"
    content = "# My Note\n\nHello World"

    create_note(
        ws_path,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    note_path = fs_join(ws_path, "notes", note_id)
    assert fs.exists(note_path)
    assert fs.exists(fs_join(note_path, "meta.json"))
    assert fs.exists(fs_join(note_path, "content.json"))
    assert fs.exists(fs_join(note_path, "history", "index.json"))

    # Check content.json
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        assert data["markdown"] == content
        assert data["frontmatter"] == {}
        assert data["sections"] == {}
        assert "revision_id" in data
        assert "parent_revision_id" in data
        assert data["parent_revision_id"] is None
        assert "author" in data


def test_update_note_revision_mismatch(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note requires the correct parent_revision_id."""
    fs, ws_path = workspace_root
    note_id = "note-2"
    content = "# Note 2"
    create_note(
        ws_path,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    # Get current revision from content.json
    note_path = fs_join(ws_path, "notes", note_id)
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        current_rev = data["revision_id"]

    # Update with correct revision
    new_content = "# Note 2 Updated"
    update_note(
        ws_path,
        note_id,
        new_content,
        parent_revision_id=current_rev,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    # Get new revision
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        new_rev = data["revision_id"]

    assert new_rev != current_rev

    # Try to update with OLD revision (should fail)
    with pytest.raises(RevisionMismatchError):
        update_note(
            ws_path,
            note_id,
            "Should fail",
            parent_revision_id=current_rev,
            integrity_provider=fake_integrity_provider,
            fs=fs,
        )

    # Try to update with WRONG revision
    with pytest.raises(RevisionMismatchError):
        update_note(
            ws_path,
            note_id,
            "Should fail",
            parent_revision_id="wrong-rev",
            integrity_provider=fake_integrity_provider,
            fs=fs,
        )


def test_note_history_append(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note appends to history and updates index."""
    fs, ws_path = workspace_root
    note_id = "note-history"
    content_v1 = "# Version 1"
    create_note(
        ws_path,
        note_id,
        content_v1,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    note_path = fs_join(ws_path, "notes", note_id)

    # Get v1 revision
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        rev_v1 = data["revision_id"]

    # Update to v2
    content_v2 = "# Version 2"
    update_note(
        ws_path,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    # Check history index
    with fs.open(fs_join(note_path, "history", "index.json"), "r") as f:
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
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        rev_v2 = data["revision_id"]

    with fs.open(fs_join(note_path, "history", f"{rev_v2}.json"), "r") as f:
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
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that frontmatter and sections persist to storage."""
    fs, ws_path = workspace_root
    note_id = "note-structured"
    content = STRUCTURED_NOTE_CONTENT

    create_note(
        ws_path,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    note_path = fs_join(ws_path, "notes", note_id)
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)

    assert data["frontmatter"]["class"] == "meeting"
    assert data["sections"]["Date"] == "2025-11-29"
    assert data["sections"]["Summary"] == "Wrap up"


def test_note_history_diff(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that updating a note stores the diff in the history file."""
    fs, ws_path = workspace_root
    note_id = "note-diff"
    content_v1 = "Line 1\nLine 2"
    create_note(
        ws_path,
        note_id,
        content_v1,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    note_path = fs_join(ws_path, "notes", note_id)
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        rev_v1 = data["revision_id"]

    content_v2 = "Line 1\nLine 2 Modified"
    update_note(
        ws_path,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        rev_v2 = data["revision_id"]

    with fs.open(fs_join(note_path, "history", f"{rev_v2}.json"), "r") as f:
        rev_data = json.load(f)
        assert "diff" in rev_data
        assert "Line 2 Modified" in rev_data["diff"]


def test_note_author_persistence(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that the author field is persisted correctly."""
    fs, ws_path = workspace_root
    note_id = "note-author"
    content = "# Author Test"
    author = "agent-smith"

    create_note(
        ws_path,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        author=author,
        fs=fs,
    )

    note_path = fs_join(ws_path, "notes", note_id)
    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        assert data["author"] == author
        rev_id = data["revision_id"]

    with fs.open(fs_join(note_path, "history", f"{rev_id}.json"), "r") as f:
        data = json.load(f)
        assert data["author"] == author

    # Update with different author
    new_author = "neo"
    update_note(
        ws_path,
        note_id,
        "# Author Test Updated",
        parent_revision_id=rev_id,
        integrity_provider=fake_integrity_provider,
        author=new_author,
        fs=fs,
    )

    with fs.open(fs_join(note_path, "content.json"), "r") as f:
        data = json.load(f)
        assert data["author"] == new_author
        rev_id_2 = data["revision_id"]

    with fs.open(fs_join(note_path, "history", f"{rev_id_2}.json"), "r") as f:
        data = json.load(f)
        assert data["author"] == new_author


def test_list_notes_returns_properties_and_links(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Verifies that list_notes returns properties and links fields."""
    fs, ws_path = workspace_root
    note_id = "note-with-props"
    content = "# Test Note\n\nSome content"

    create_note(
        ws_path,
        note_id,
        content,
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    notes = list_notes(ws_path, fs=fs)
    assert len(notes) == 1

    note = notes[0]
    assert note["id"] == note_id
    assert "properties" in note, (
        "properties field must be present in list_notes response"
    )
    assert "links" in note, "links field must be present in list_notes response"
    assert isinstance(note["properties"], dict)
    assert isinstance(note["links"], list)
