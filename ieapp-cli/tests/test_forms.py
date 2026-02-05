"""Tests for form migration and column types."""

import uuid
from pathlib import Path

from ieapp.entries import create_entry, get_entry
from ieapp.forms import list_column_types, migrate_form, upsert_form
from ieapp.indexer import extract_properties
from ieapp.space import create_space


def test_list_column_types() -> None:
    """Test available column types listing (REQ-FORM-001)."""
    types = list_column_types()
    assert isinstance(types, list)
    assert "string" in types
    assert "number" in types
    assert "double" in types
    assert "float" in types
    assert "integer" in types
    assert "long" in types
    assert "boolean" in types
    assert "date" in types
    assert "time" in types
    assert "timestamp" in types
    assert "timestamp_tz" in types
    assert "timestamp_ns" in types
    assert "timestamp_tz_ns" in types
    assert "uuid" in types
    assert "binary" in types
    assert "list" in types
    assert "markdown" in types


def test_migrate_form_add_column_with_default(tmp_path: Path) -> None:
    """Test migrating form by adding a column with a default value (REQ-FORM-002)."""
    # Setup space
    create_space(str(tmp_path), "ws")
    ws_path = str(tmp_path / "spaces" / "ws")

    # Define initial form
    initial_form = {
        "name": "project",
        "template": "# Project\n\n## summary\n\n## status\n",
        "fields": {
            "summary": {"type": "string"},
            "status": {"type": "string"},
        },
    }
    upsert_form(ws_path, initial_form)

    # Create a entry
    entry_id = str(uuid.uuid4())
    content = (
        "---\nform: project\n---\n# Project Alpha\n\n## summary\nAlpha\n\n"
        "## status\nActive\n"
    )

    create_entry(
        ws_path,
        entry_id,
        content=content,
    )

    # Define new form (adding 'priority')
    new_form = {
        "name": "project",
        "template": "# Project\n\n## summary\n\n## status\n\n## priority\n",
        "fields": {
            "summary": {"type": "string"},
            "status": {"type": "string"},
            "priority": {"type": "string"},
        },
    }

    # Migrate with default value for new column
    count = migrate_form(
        ws_path,
        new_form,
        strategies={"priority": "Medium"},
    )

    assert count == 1

    # Verify
    updated_entry = get_entry(ws_path, entry_id)
    props = extract_properties(updated_entry["content"])

    assert props.get("priority") == "Medium"
    # Ensure markdown was updated
    assert "## priority" in updated_entry["content"]
    assert "Medium" in updated_entry["content"]


def test_migrate_form_remove_column(tmp_path: Path) -> None:
    """Test migrating form by removing a column (REQ-FORM-002)."""
    # Setup space
    create_space(str(tmp_path), "ws")
    ws_path = str(tmp_path / "spaces" / "ws")

    # Initial form with extra field
    initial_form = {
        "name": "task",
        "template": "# Task\n\n## summary\n\n## old_field\n",
        "fields": {
            "summary": {"type": "string"},
            "old_field": {"type": "string"},
        },
    }
    upsert_form(ws_path, initial_form)

    entry_id = str(uuid.uuid4())
    content = (
        "---\nform: task\n---\n# Task 1\n\n## summary\nDo it\n\n"
        "## old_field\nDelete me\n"
    )

    create_entry(
        ws_path,
        entry_id,
        content=content,
    )

    # New form without old_field
    new_form = {
        "name": "task",
        "template": "# Task\n\n## summary\n",
        "fields": {
            "summary": {"type": "string"},
        },
    }

    migrate_form(
        ws_path,
        new_form,
        strategies={"old_field": None},
    )

    updated_entry = get_entry(ws_path, entry_id)
    props = extract_properties(updated_entry["content"])
    assert "old_field" not in props
    assert "Delete me" not in updated_entry["content"]
