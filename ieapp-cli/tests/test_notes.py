"""Tests for notes management.

REQ-NOTE-001: Note creation.
REQ-NOTE-002: Optimistic concurrency via revisions.
REQ-NOTE-003: Note update.
REQ-NOTE-004: Note deletion.
REQ-NOTE-005: Note history.
REQ-NOTE-006: Structured data extraction.
REQ-NOTE-007: Properties and links in list response.
REQ-NOTE-008: Attachments upload & linking.
"""

from typing import Any

import fsspec
import pytest

from ieapp.classes import upsert_class
from ieapp.notes import (
    RevisionMismatchError,
    create_note,
    get_note,
    get_note_history,
    get_note_revision,
    list_notes,
    update_note,
)
from ieapp.utils import fs_join
from ieapp.workspace import create_workspace

DEFAULT_CLASS = "Note"
MEETING_CLASS = "Meeting"

STRUCTURED_NOTE_CONTENT = """---
class: Meeting
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
    upsert_class(
        ws_path,
        {
            "name": DEFAULT_CLASS,
            "template": "# Note\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )
    upsert_class(
        ws_path,
        {
            "name": MEETING_CLASS,
            "template": "# Meeting\n\n## Date\n\n## Summary\n",
            "fields": {
                "Date": {"type": "date"},
                "Summary": {"type": "string"},
            },
        },
    )
    return fs, ws_path


def test_create_note_basic(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that creating a note generates the required file structure."""
    _fs, ws_path = workspace_root
    note_id = "note-1"
    content = """---
class: Note
---
# My Note

Hello World
"""

    create_note(
        ws_path,
        note_id,
        content,
    )

    data = get_note(ws_path, note_id)
    assert data["id"] == note_id
    assert data["class"] == DEFAULT_CLASS
    assert "revision_id" in data
    assert "# My Note" in data["content"]


def test_update_note_revision_mismatch(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a note requires the correct parent_revision_id."""
    _fs, ws_path = workspace_root
    note_id = "note-2"
    content = """---
class: Note
---
# Note 2
"""
    create_note(
        ws_path,
        note_id,
        content,
    )

    data = get_note(ws_path, note_id)
    current_rev = data["revision_id"]

    # Update with correct revision
    new_content = """---
class: Note
---
# Note 2 Updated
"""
    update_note(
        ws_path,
        note_id,
        new_content,
        parent_revision_id=current_rev,
    )

    # Get new revision
    data = get_note(ws_path, note_id)
    new_rev = data["revision_id"]

    assert new_rev != current_rev

    # Try to update with OLD revision (should fail)
    with pytest.raises(RevisionMismatchError):
        update_note(
            ws_path,
            note_id,
            "Should fail",
            parent_revision_id=current_rev,
        )

    # Try to update with WRONG revision
    with pytest.raises(RevisionMismatchError):
        update_note(
            ws_path,
            note_id,
            "Should fail",
            parent_revision_id="wrong-rev",
        )


def test_note_history_append(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a note appends to revision history."""
    _fs, ws_path = workspace_root
    note_id = "note-history"
    content_v1 = """---
class: Note
---
# Version 1
"""
    create_note(
        ws_path,
        note_id,
        content_v1,
    )

    data = get_note(ws_path, note_id)
    rev_v1 = data["revision_id"]

    # Update to v2
    content_v2 = """---
class: Note
---
# Version 2
"""
    update_note(
        ws_path,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
    )

    history = get_note_history(ws_path, note_id)
    revisions = history.get("revisions", [])
    assert len(revisions) == HISTORY_LENGTH

    data = get_note(ws_path, note_id)
    rev_v2 = data["revision_id"]

    rev_data = get_note_revision(ws_path, note_id, rev_v2)
    assert rev_data["revision_id"] == rev_v2
    assert rev_data["parent_revision_id"] == rev_v1


def test_markdown_sections_persist(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that frontmatter and sections persist to storage."""
    _fs, ws_path = workspace_root
    note_id = "note-structured"
    content = STRUCTURED_NOTE_CONTENT

    create_note(
        ws_path,
        note_id,
        content,
    )

    data = get_note(ws_path, note_id)

    assert data["class"] == MEETING_CLASS
    assert data["sections"]["Date"] == "2025-11-29"
    assert data["sections"]["Summary"] == "Wrap up"


def test_note_history_diff(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a note stores a revision entry."""
    _fs, ws_path = workspace_root
    note_id = "note-diff"
    content_v1 = """---
class: Note
---
# Note Diff

Line 1
Line 2
"""
    create_note(
        ws_path,
        note_id,
        content_v1,
    )

    data = get_note(ws_path, note_id)
    rev_v1 = data["revision_id"]

    content_v2 = """---
class: Note
---
# Note Diff

Line 1
Line 2 Modified
"""
    update_note(
        ws_path,
        note_id,
        content_v2,
        parent_revision_id=rev_v1,
    )

    data = get_note(ws_path, note_id)
    rev_v2 = data["revision_id"]

    history = get_note_history(ws_path, note_id)
    revisions = history.get("revisions", [])
    assert len(revisions) == HISTORY_LENGTH

    rev_data = get_note_revision(ws_path, note_id, rev_v2)
    assert rev_data["revision_id"] == rev_v2
    assert rev_data["note_id"] == note_id


def test_note_author_persistence(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that the author field is persisted correctly."""
    _fs, ws_path = workspace_root
    note_id = "note-author"
    content = """---
class: Note
---
# Author Test
"""
    author = "agent-smith"

    create_note(
        ws_path,
        note_id,
        content,
        author=author,
    )

    history = get_note_history(ws_path, note_id)
    revisions = history.get("revisions", [])
    assert revisions
    rev_id = revisions[-1]["revision_id"]
    data = get_note_revision(ws_path, note_id, rev_id)
    assert data["author"] == author

    # Update with different author
    new_author = "neo"
    update_note(
        ws_path,
        note_id,
        """---
class: Note
---
# Author Test Updated
""",
        parent_revision_id=rev_id,
        author=new_author,
    )

    history = get_note_history(ws_path, note_id)
    revisions = history.get("revisions", [])
    rev_id_2 = revisions[-1]["revision_id"]
    data = get_note_revision(ws_path, note_id, rev_id_2)
    assert data["author"] == new_author


def test_list_notes_returns_properties_and_links(
    workspace_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that list_notes returns properties and links fields."""
    _fs, ws_path = workspace_root
    note_id = "note-with-props"
    content = """---
class: Note
---
# Test Note

Some content
"""

    create_note(
        ws_path,
        note_id,
        content,
    )

    notes = list_notes(ws_path)
    assert len(notes) == 1

    note = notes[0]
    assert note["id"] == note_id
    assert "properties" in note, (
        "properties field must be present in list_notes response"
    )
    assert "links" in note, "links field must be present in list_notes response"
    assert isinstance(note["properties"], dict)
    assert isinstance(note["links"], list)
