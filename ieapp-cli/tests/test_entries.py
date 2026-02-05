"""Tests for entries management.

REQ-ENTRY-001: Entry creation.
REQ-ENTRY-002: Optimistic concurrency via revisions.
REQ-ENTRY-003: Entry update.
REQ-ENTRY-004: Entry deletion.
REQ-ENTRY-005: Entry history.
REQ-ENTRY-006: Structured data extraction.
REQ-ENTRY-007: Properties and links in list response.
REQ-ENTRY-008: Assets upload & linking.
"""

from typing import Any

import fsspec
import pytest

from ieapp.entries import (
    RevisionMismatchError,
    create_entry,
    get_entry,
    get_entry_history,
    get_entry_revision,
    list_entries,
    update_entry,
)
from ieapp.forms import upsert_form
from ieapp.space import create_space
from ieapp.utils import fs_join

DEFAULT_FORM = "Entry"
MEETING_FORM = "Meeting"

STRUCTURED_ENTRY_CONTENT = """---
form: Meeting
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
def space_root(fs_impl: tuple[fsspec.AbstractFileSystem, str]) -> tuple[Any, str]:
    """Create a temporary space for testing."""
    fs, root = fs_impl
    ws_id = "test-space"
    create_space(root, ws_id, fs=fs)
    ws_path = fs_join(root, "spaces", ws_id)
    upsert_form(
        ws_path,
        {
            "name": DEFAULT_FORM,
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )
    upsert_form(
        ws_path,
        {
            "name": MEETING_FORM,
            "template": "# Meeting\n\n## Date\n\n## Summary\n",
            "fields": {
                "Date": {"type": "date"},
                "Summary": {"type": "string"},
            },
        },
    )
    return fs, ws_path


def test_create_entry_basic(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that creating a entry generates the required file structure."""
    _fs, ws_path = space_root
    entry_id = "entry-1"
    content = """---
form: Entry
---
# My Entry

Hello World
"""

    create_entry(
        ws_path,
        entry_id,
        content,
    )

    data = get_entry(ws_path, entry_id)
    assert data["id"] == entry_id
    assert data["form"] == DEFAULT_FORM
    assert "revision_id" in data
    assert "# My Entry" in data["content"]


def test_update_entry_revision_mismatch(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a entry requires the correct parent_revision_id."""
    _fs, ws_path = space_root
    entry_id = "entry-2"
    content = """---
form: Entry
---
# Entry 2
"""
    create_entry(
        ws_path,
        entry_id,
        content,
    )

    data = get_entry(ws_path, entry_id)
    current_rev = data["revision_id"]

    # Update with correct revision
    new_content = """---
form: Entry
---
# Entry 2 Updated
"""
    update_entry(
        ws_path,
        entry_id,
        new_content,
        parent_revision_id=current_rev,
    )

    # Get new revision
    data = get_entry(ws_path, entry_id)
    new_rev = data["revision_id"]

    assert new_rev != current_rev

    # Try to update with OLD revision (should fail)
    with pytest.raises(RevisionMismatchError):
        update_entry(
            ws_path,
            entry_id,
            "Should fail",
            parent_revision_id=current_rev,
        )

    # Try to update with WRONG revision
    with pytest.raises(RevisionMismatchError):
        update_entry(
            ws_path,
            entry_id,
            "Should fail",
            parent_revision_id="wrong-rev",
        )


def test_entry_history_append(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a entry appends to revision history."""
    _fs, ws_path = space_root
    entry_id = "entry-history"
    content_v1 = """---
form: Entry
---
# Version 1
"""
    create_entry(
        ws_path,
        entry_id,
        content_v1,
    )

    data = get_entry(ws_path, entry_id)
    rev_v1 = data["revision_id"]

    # Update to v2
    content_v2 = """---
form: Entry
---
# Version 2
"""
    update_entry(
        ws_path,
        entry_id,
        content_v2,
        parent_revision_id=rev_v1,
    )

    history = get_entry_history(ws_path, entry_id)
    revisions = history.get("revisions", [])
    assert len(revisions) == HISTORY_LENGTH

    data = get_entry(ws_path, entry_id)
    rev_v2 = data["revision_id"]

    rev_data = get_entry_revision(ws_path, entry_id, rev_v2)
    assert rev_data["revision_id"] == rev_v2
    assert rev_data["parent_revision_id"] == rev_v1


def test_markdown_sections_persist(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that frontmatter and sections persist to storage."""
    _fs, ws_path = space_root
    entry_id = "entry-structured"
    content = STRUCTURED_ENTRY_CONTENT

    create_entry(
        ws_path,
        entry_id,
        content,
    )

    data = get_entry(ws_path, entry_id)

    assert data["form"] == MEETING_FORM
    assert data["sections"]["Date"] == "2025-11-29"
    assert data["sections"]["Summary"] == "Wrap up"


def test_entry_history_diff(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that updating a entry stores a revision entry."""
    _fs, ws_path = space_root
    entry_id = "entry-diff"
    content_v1 = """---
form: Entry
---
# Entry Diff

Line 1
Line 2
"""
    create_entry(
        ws_path,
        entry_id,
        content_v1,
    )

    data = get_entry(ws_path, entry_id)
    rev_v1 = data["revision_id"]

    content_v2 = """---
form: Entry
---
# Entry Diff

Line 1
Line 2 Modified
"""
    update_entry(
        ws_path,
        entry_id,
        content_v2,
        parent_revision_id=rev_v1,
    )

    data = get_entry(ws_path, entry_id)
    rev_v2 = data["revision_id"]

    history = get_entry_history(ws_path, entry_id)
    revisions = history.get("revisions", [])
    assert len(revisions) == HISTORY_LENGTH

    rev_data = get_entry_revision(ws_path, entry_id, rev_v2)
    assert rev_data["revision_id"] == rev_v2
    assert rev_data["entry_id"] == entry_id


def test_entry_author_persistence(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that the author field is persisted correctly."""
    _fs, ws_path = space_root
    entry_id = "entry-author"
    content = """---
form: Entry
---
# Author Test
"""
    author = "agent-smith"

    create_entry(
        ws_path,
        entry_id,
        content,
        author=author,
    )

    history = get_entry_history(ws_path, entry_id)
    revisions = history.get("revisions", [])
    assert revisions
    rev_id = revisions[-1]["revision_id"]
    data = get_entry_revision(ws_path, entry_id, rev_id)
    assert data["author"] == author

    # Update with different author
    new_author = "neo"
    update_entry(
        ws_path,
        entry_id,
        """---
form: Entry
---
# Author Test Updated
""",
        parent_revision_id=rev_id,
        author=new_author,
    )

    history = get_entry_history(ws_path, entry_id)
    revisions = history.get("revisions", [])
    rev_id_2 = revisions[-1]["revision_id"]
    data = get_entry_revision(ws_path, entry_id, rev_id_2)
    assert data["author"] == new_author


def test_list_entries_returns_properties_and_links(
    space_root: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Verifies that list_entries returns properties and links fields."""
    _fs, ws_path = space_root
    entry_id = "entry-with-props"
    content = """---
form: Entry
---
# Test Entry

Some content
"""

    create_entry(
        ws_path,
        entry_id,
        content,
    )

    entries = list_entries(ws_path)
    assert len(entries) == 1

    entry = entries[0]
    assert entry["id"] == entry_id
    assert "properties" in entry, (
        "properties field must be present in list_entries response"
    )
    assert "links" in entry, "links field must be present in list_entries response"
    assert isinstance(entry["properties"], dict)
    assert isinstance(entry["links"], list)
