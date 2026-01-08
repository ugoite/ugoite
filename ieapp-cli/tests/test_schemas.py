"""Tests for schema migration and column types."""

import uuid
from pathlib import Path

from ieapp.indexer import extract_properties
from ieapp.notes import create_note, get_note
from ieapp.schemas import list_column_types, migrate_schema, upsert_schema
from ieapp.workspace import create_workspace


def test_list_column_types() -> None:
    """Test available column types listing (REQ-SCH-001)."""
    types = list_column_types()
    assert isinstance(types, list)
    assert "string" in types
    assert "number" in types
    assert "date" in types
    assert "list" in types
    assert "markdown" in types


def test_migrate_schema_add_column_with_default(tmp_path: Path) -> None:
    """Test migrating schema by adding a column with a default value (REQ-SCH-002)."""
    # Setup workspace
    create_workspace(str(tmp_path), "ws")
    ws_path = str(tmp_path / "workspaces" / "ws")

    # Define initial schema
    initial_schema = {
        "name": "project",
        "fields": {
            "title": {"type": "string"},
            "status": {"type": "string"},
        },
    }
    upsert_schema(ws_path, initial_schema)

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

    # Define new schema (adding 'priority')
    new_schema = {
        "name": "project",
        "fields": {
            "title": {"type": "string"},
            "status": {"type": "string"},
            "priority": {"type": "string"},
        },
    }

    # Migrate with default value for new column
    count = migrate_schema(
        ws_path,
        new_schema,
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


def test_migrate_schema_remove_column(tmp_path: Path) -> None:
    """Test migrating schema by removing a column (REQ-SCH-002)."""
    # Setup workspace
    create_workspace(str(tmp_path), "ws")
    ws_path = str(tmp_path / "workspaces" / "ws")

    # Initial schema with extra field
    initial_schema = {
        "name": "task",
        "fields": {
            "title": {"type": "string"},
            "old_field": {"type": "string"},
        },
    }
    upsert_schema(ws_path, initial_schema)

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

    # New schema without old_field
    new_schema = {
        "name": "task",
        "fields": {
            "title": {"type": "string"},
        },
    }

    migrate_schema(
        ws_path,
        new_schema,
        strategies={"old_field": None},
    )

    updated_note = get_note(ws_path, note_id)
    props = extract_properties(updated_note["content"])
    assert "old_field" not in props
    assert "Delete me" not in updated_note["content"]
