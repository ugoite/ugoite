import json
import fsspec
from ieapp.indexer import (
    extract_properties,
    validate_properties,
    aggregate_stats,
    Indexer,
    query_index,
)


def test_extract_properties_h2_sections():
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


def test_extract_properties_precedence():
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


def test_validate_properties_missing_required():
    schema = {
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        }
    }

    # Missing Date
    properties = {"Attendees": "- Alice"}

    warnings = validate_properties(properties, schema)
    assert len(warnings) == 1
    assert warnings[0]["code"] == "missing_field"
    assert warnings[0]["field"] == "Date"


def test_validate_properties_valid():
    schema = {"fields": {"Date": {"type": "date", "required": True}}}
    properties = {"Date": "2025-10-27"}
    warnings = validate_properties(properties, schema)
    assert len(warnings) == 0


def test_aggregate_stats():
    index_data = {
        "note1": {"class": "meeting", "Date": "2025-10-27"},
        "note2": {"class": "meeting", "Date": "2025-10-28"},
        "note3": {"class": "task", "status": "todo"},
        "note4": {"title": "Just a note"},  # No class
    }

    stats = aggregate_stats(index_data)

    assert stats["class_stats"]["meeting"]["count"] == 2
    assert stats["class_stats"]["task"]["count"] == 1
    assert stats["class_stats"]["_uncategorized"]["count"] == 1

    assert stats["total_notes"] == 4


def test_indexer_run_once():
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
        "markdown": "---\nclass: meeting\n---\n# Sync\n\n## Date\n2025-10-27"
    }
    with fs.open(f"{workspace_path}/notes/note1/content.json", "w") as f:
        json.dump(note1_content, f)

    # Create Note 2 (Invalid Meeting)
    note2_content = {"markdown": "---\nclass: meeting\n---\n# Sync\n"}
    with fs.open(f"{workspace_path}/notes/note2/content.json", "w") as f:
        json.dump(note2_content, f)

    indexer = Indexer(workspace_path, fs=fs)
    indexer.run_once()

    # Check index.json
    with fs.open(f"{workspace_path}/index/index.json", "r") as f:
        index_data = json.load(f)

    assert "note1" in index_data
    assert index_data["note1"]["class"] == "meeting"
    assert index_data["note1"]["Date"] == "2025-10-27"
    assert (
        "validation_warnings" not in index_data["note1"]
        or not index_data["note1"]["validation_warnings"]
    )

    assert "note2" in index_data
    assert index_data["note2"]["class"] == "meeting"
    assert len(index_data["note2"]["validation_warnings"]) > 0

    # Check stats.json
    with fs.open(f"{workspace_path}/index/stats.json", "r") as f:
        stats_data = json.load(f)

    assert stats_data["class_stats"]["meeting"]["count"] == 2


def test_query_index():
    fs = fsspec.filesystem("memory")
    workspace_path = "/test_workspace"
    fs.makedirs(f"{workspace_path}/index", exist_ok=True)

    index_data = {
        "note1": {"class": "meeting", "Date": "2025-10-27"},
        "note2": {"class": "meeting", "Date": "2025-10-28"},
        "note3": {"class": "task", "status": "todo"},
    }

    with fs.open(f"{workspace_path}/index/index.json", "w") as f:
        json.dump(index_data, f)

    results = query_index(workspace_path, {"class": "meeting"}, fs=fs)
    assert len(results) == 2
    assert "note1" in results
    assert "note2" in results

    results = query_index(workspace_path, {"class": "task"}, fs=fs)
    assert len(results) == 1
    assert "note3" in results
