"""Tests for class migration and column types."""

import uuid
from pathlib import Path

from ieapp.classes import list_column_types, migrate_class, upsert_class
from ieapp.indexer import extract_properties
from ieapp.notes import create_note, get_note
from ieapp.workspace import create_workspace


def test_list_column_types() -> None:
    """Test available column types listing (REQ-CLS-001)."""
    types = list_column_types()
    assert isinstance(types, list)
    assert "string" in types
    assert "number" in types
    assert "date" in types
    assert "list" in types
    assert "markdown" in types


def test_migrate_class_add_column_with_default(tmp_path: Path) -> None:
    """Test migrating class by adding a column with a default value (REQ-CLS-002)."""
    # Setup workspace
    create_workspace(str(tmp_path), "ws")
    ws_path = str(tmp_path / "workspaces" / "ws")

    # Define initial class
    initial_class = {
        "name": "project",
        "template": "# Project\n\n## title\n\n## status\n",
        "fields": {
            "title": {"type": "string"},
            "status": {"type": "string"},
        },
    }
    upsert_class(ws_path, initial_class)

    # Create a note
    note_id = str(uuid.uuid4())
    content = (
        "---\nclass: project\n---\n# Project Alpha\n\n## title\nAlpha\n\n"
        "## status\nActive\n"
    )

    create_note(
        ws_path,
        note_id,
        content=content,
    )

    # Define new class (adding 'priority')
    new_class = {
        "name": "project",
        "template": "# Project\n\n## title\n\n## status\n\n## priority\n",
        "fields": {
            "title": {"type": "string"},
            "status": {"type": "string"},
            "priority": {"type": "string"},
        },
    }

    # Migrate with default value for new column
    count = migrate_class(
        ws_path,
        new_class,
        strategies={"priority": "Medium"},
    )

    assert count == 1

    # Verify
    updated_note = get_note(ws_path, note_id)
    props = extract_properties(updated_note["content"])

    assert props.get("priority") == "Medium"
    # Ensure markdown was updated
    assert "## priority" in updated_note["content"]
    assert "Medium" in updated_note["content"]


def test_migrate_class_remove_column(tmp_path: Path) -> None:
    """Test migrating class by removing a column (REQ-CLS-002)."""
    # Setup workspace
    create_workspace(str(tmp_path), "ws")
    ws_path = str(tmp_path / "workspaces" / "ws")

    # Initial class with extra field
    initial_class = {
        "name": "task",
        "template": "# Task\n\n## title\n\n## old_field\n",
        "fields": {
            "title": {"type": "string"},
            "old_field": {"type": "string"},
        },
    }
    upsert_class(ws_path, initial_class)

    note_id = str(uuid.uuid4())
    content = (
        "---\nclass: task\n---\n# Task 1\n\n## title\nDo it\n\n"
        "## old_field\nDelete me\n"
    )

    create_note(
        ws_path,
        note_id,
        content=content,
    )

    # New class without old_field
    new_class = {
        "name": "task",
        "template": "# Task\n\n## title\n",
        "fields": {
            "title": {"type": "string"},
        },
    }

    migrate_class(
        ws_path,
        new_class,
        strategies={"old_field": None},
    )

    updated_note = get_note(ws_path, note_id)
    props = extract_properties(updated_note["content"])
    assert "old_field" not in props
    assert "Delete me" not in updated_note["content"]
