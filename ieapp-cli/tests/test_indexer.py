"""Tests for the ieapp indexer utilities."""

import json
from typing import Callable

import fsspec
import pytest

from ieapp.indexer import (
    Indexer,
    aggregate_stats,
    extract_properties,
    query_index,
    validate_properties,
)

EXPECTED_MEETING_COUNT = 2
EXPECTED_TASK_COUNT = 1
EXPECTED_UNCATEGORIZED_COUNT = 1
EXPECTED_TAG_ALPHA_COUNT = 2
EXPECTED_TOTAL_NOTES = 4
EXPECTED_INDEXED_NOTES = 2
EXPECTED_MEETING_RESULTS = 2
EXPECTED_SINGLE_RESULT = 1
EXPECTED_WATCH_CALLBACKS = 2


def test_extract_properties_h2_sections() -> None:
    """Ensure Markdown properties merge frontmatter and H2 sections."""
    markdown = """---
class: meeting
status: draft
---
# Weekly Sync

## Date
2025-10-27

## Attendees
- Alice
- Bob

## Agenda
1. Review PRs
2. Plan next sprint
"""
    properties = extract_properties(markdown)

    # Check frontmatter
    assert properties["class"] == "meeting"
    assert properties["status"] == "draft"

    # Check H2 sections
    assert properties["Date"] == "2025-10-27"
    assert properties["Attendees"] == "- Alice\n- Bob"
    assert properties["Agenda"] == "1. Review PRs\n2. Plan next sprint"


def test_extract_properties_precedence() -> None:
    """Ensure section values override frontmatter keys."""
    # Section should override frontmatter
    markdown = """---
title: Frontmatter Title
---
# Main Title

## title
Section Title
"""
    properties = extract_properties(markdown)
    assert properties["title"] == "Section Title"


def test_validate_properties_missing_required() -> None:
    """Emit missing_field warning when required keys are absent."""
    schema = {
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        },
    }

    # Missing Date
    properties = {"Attendees": "- Alice"}

    warnings = validate_properties(properties, schema)
    assert len(warnings) == EXPECTED_TASK_COUNT
    assert warnings[0]["code"] == "missing_field"
    assert warnings[0]["field"] == "Date"


def test_validate_properties_valid() -> None:
    """Return zero warnings when required data is present."""
    schema = {"fields": {"Date": {"type": "date", "required": True}}}
    properties = {"Date": "2025-10-27"}
    warnings = validate_properties(properties, schema)
    assert len(warnings) == 0


def test_aggregate_stats() -> None:
    """Aggregate counts for classes, tags, and totals."""
    index_data = {
        "note1": {"class": "meeting", "properties": {}, "tags": ["alpha"]},
        "note2": {"class": "meeting", "properties": {}},
        "note3": {"class": "task", "properties": {}, "tags": ["alpha"]},
        "note4": {"title": "Just a note", "properties": {}},  # No class
    }

    stats = aggregate_stats(index_data)

    assert stats["class_stats"]["meeting"]["count"] == EXPECTED_MEETING_COUNT
    assert stats["class_stats"]["task"]["count"] == EXPECTED_TASK_COUNT
    assert (
        stats["class_stats"]["_uncategorized"]["count"] == EXPECTED_UNCATEGORIZED_COUNT
    )
    assert stats["tag_counts"]["alpha"] == EXPECTED_TAG_ALPHA_COUNT
    assert stats["note_count"] == EXPECTED_TOTAL_NOTES


def test_indexer_run_once() -> None:
    """Index all notes and persist stats in memory fs."""
    fs = fsspec.filesystem("memory")
    workspace_path = "/test_workspace"
    fs.makedirs(f"{workspace_path}/notes/note1", exist_ok=True)
    fs.makedirs(f"{workspace_path}/notes/note2", exist_ok=True)
    fs.makedirs(f"{workspace_path}/index", exist_ok=True)
    fs.makedirs(f"{workspace_path}/schemas", exist_ok=True)

    # Create Schema
    schema = {"fields": {"Date": {"type": "date", "required": True}}}
    with fs.open(f"{workspace_path}/schemas/meeting.json", "w") as f:
        json.dump(schema, f)

    # Create Note 1 (Valid Meeting)
    note1_content = {
        "markdown": "---\nclass: meeting\n---\n# Sync\n\n## Date\n2025-10-27",
    }
    with fs.open(f"{workspace_path}/notes/note1/content.json", "w") as f:
        json.dump(note1_content, f)
    note1_meta = {
        "id": "note1",
        "title": "Sync",
        "class": "meeting",
        "updated_at": 111.0,
        "tags": ["alpha"],
        "links": [],
        "canvas_position": {},
        "integrity": {"checksum": "aaa"},
    }
    with fs.open(f"{workspace_path}/notes/note1/meta.json", "w") as f:
        json.dump(note1_meta, f)

    # Create Note 2 (Invalid Meeting)
    note2_content = {"markdown": "---\nclass: meeting\n---\n# Sync\n"}
    with fs.open(f"{workspace_path}/notes/note2/content.json", "w") as f:
        json.dump(note2_content, f)
    note2_meta = {
        "id": "note2",
        "title": "Sync",
        "class": "meeting",
        "updated_at": 112.0,
        "tags": [],
        "links": [],
        "canvas_position": {},
        "integrity": {"checksum": "bbb"},
    }
    with fs.open(f"{workspace_path}/notes/note2/meta.json", "w") as f:
        json.dump(note2_meta, f)

    indexer = Indexer(workspace_path, fs=fs)
    indexer.run_once()

    # Check index.json
    with fs.open(f"{workspace_path}/index/index.json", "r") as f:
        index_data = json.load(f)

    assert "notes" in index_data
    note1 = index_data["notes"]["note1"]
    assert note1["class"] == "meeting"
    assert note1["properties"]["Date"] == "2025-10-27"
    assert note1["validation_warnings"] == []
    assert note1["tags"] == ["alpha"]
    assert note1["checksum"] == "aaa"

    note2 = index_data["notes"]["note2"]
    assert note2["class"] == "meeting"
    assert len(note2["validation_warnings"]) > 0

    # Check stats.json
    with fs.open(f"{workspace_path}/index/stats.json", "r") as f:
        stats_data = json.load(f)

    assert stats_data["class_stats"]["meeting"]["count"] == EXPECTED_MEETING_COUNT
    assert stats_data["note_count"] == EXPECTED_INDEXED_NOTES
    assert "last_indexed" in stats_data


def test_query_index() -> None:
    """Filter cached index by class and derived properties."""
    fs = fsspec.filesystem("memory")
    workspace_path = "/test_workspace"
    fs.makedirs(f"{workspace_path}/index", exist_ok=True)

    index_data = {
        "notes": {
            "note1": {
                "id": "note1",
                "class": "meeting",
                "properties": {"Date": "2025-10-27"},
            },
            "note2": {
                "id": "note2",
                "class": "meeting",
                "properties": {"Date": "2025-10-28"},
            },
            "note3": {
                "id": "note3",
                "class": "task",
                "properties": {"status": "todo"},
            },
        },
    }

    with fs.open(f"{workspace_path}/index/index.json", "w") as f:
        json.dump(index_data, f)

    results = query_index(workspace_path, {"class": "meeting"}, fs=fs)
    assert len(results) == EXPECTED_MEETING_RESULTS
    ids = {note["id"] for note in results}
    assert ids == {"note1", "note2"}

    results = query_index(workspace_path, {"class": "task"}, fs=fs)
    assert len(results) == EXPECTED_SINGLE_RESULT
    assert results[0]["id"] == "note3"

    # Filtering by properties should work as well
    results = query_index(workspace_path, {"status": "todo"}, fs=fs)
    assert len(results) == EXPECTED_SINGLE_RESULT
    assert results[0]["id"] == "note3"


def test_indexer_watch_loop_triggers_run(monkeypatch: pytest.MonkeyPatch) -> None:
    """Ensure watch loop invokes run_once for each file-system event."""
    fs = fsspec.filesystem("memory")
    indexer = Indexer("/test_workspace", fs=fs)

    calls: list[int] = []

    def fake_run_once() -> None:
        calls.append(1)

    monkeypatch.setattr(indexer, "run_once", fake_run_once)

    def wait_for_changes(callback: Callable[[], None]) -> None:
        callback()
        callback()

    indexer.watch(wait_for_changes)

    assert len(calls) == EXPECTED_WATCH_CALLBACKS
