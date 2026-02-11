"""Tests for the ugoite indexer utilities.

REQ-IDX-001: Structured cache via live indexer.
REQ-IDX-002: Form validation warnings.
REQ-IDX-003: Structured queries.
REQ-IDX-004: Inverted index generation.
REQ-IDX-005: Word count calculation.
REQ-IDX-006: Indexing via watch loop.
"""

import json
from collections.abc import Callable

import fsspec
import pytest
import ugoite_core

import ugoite.indexer as indexer_module
from ugoite.indexer import (
    Indexer,
    aggregate_stats,
    build_inverted_index,
    compute_word_count,
    extract_properties,
    query_index,
    validate_properties,
)
from ugoite.utils import fs_join

EXPECTED_TASK_COUNT = 1
EXPECTED_MEETING_RESULTS = 2
EXPECTED_SINGLE_RESULT = 1
EXPECTED_WATCH_CALLBACKS = 2
EXPECTED_WORD_COUNT = 11
EXPECTED_FIELD_COUNT_2 = 2


def test_extract_properties_h2_sections() -> None:
    """Ensure Markdown properties merge frontmatter and H2 sections."""
    markdown = """---
form: meeting
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
    assert properties["form"] == "meeting"
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
    entry_form = {
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        },
    }

    # Missing Date
    properties = {"Attendees": "- Alice"}

    _, warnings = validate_properties(properties, entry_form)
    assert len(warnings) == EXPECTED_TASK_COUNT
    assert warnings[0]["code"] == "missing_field"
    assert warnings[0]["field"] == "Date"


def test_validate_properties_valid() -> None:
    """Return zero warnings when required data is present."""
    entry_form = {"fields": {"Date": {"type": "date", "required": True}}}
    properties = {"Date": "2025-10-27"}
    _, warnings = validate_properties(properties, entry_form)
    assert len(warnings) == 0


def test_indexer_run_once(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Index all entries and trigger core reindexing."""
    fs, root = fs_impl
    space_path = fs_join(root, "spaces", "test-space")
    calls: dict[str, object] = {}

    def fake_run_async(func: object, *args: object, **kwargs: object) -> None:
        calls["func"] = func
        calls["args"] = args
        calls["kwargs"] = kwargs

    monkeypatch.setattr(indexer_module, "run_async", fake_run_async)

    indexer = Indexer(space_path, fs=fs)
    indexer.run_once()

    assert calls["func"] is ugoite_core.reindex_all
    args = calls["args"]
    assert isinstance(args, tuple)
    assert args[1] == "test-space"
    config = args[0]
    assert isinstance(config, dict)
    assert "uri" in config


def test_query_index(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Filter cached index by form and derived properties."""
    fs, root = fs_impl
    space_path = fs_join(root, "spaces", "test_space")

    def fake_run_async(
        func: object,
        config: dict[str, str],
        space_id: str,
        payload: str,
    ) -> list[dict[str, object]]:
        assert func is ugoite_core.query_index
        assert space_id == "test_space"
        assert "uri" in config
        filters = json.loads(payload)
        if filters == {"form": "meeting"}:
            return [
                {"id": "entry1", "form": "meeting"},
                {"id": "entry2", "form": "meeting"},
            ]
        if filters == {"form": "task"}:
            return [{"id": "entry3", "form": "task"}]
        if filters == {"status": "todo"}:
            return [{"id": "entry3", "form": "task"}]
        return []

    monkeypatch.setattr(indexer_module, "run_async", fake_run_async)

    results = query_index(space_path, {"form": "meeting"}, fs=fs)
    assert len(results) == EXPECTED_MEETING_RESULTS
    ids = {entry["id"] for entry in results}
    assert ids == {"entry1", "entry2"}

    results = query_index(space_path, {"form": "task"}, fs=fs)
    assert len(results) == EXPECTED_SINGLE_RESULT
    assert results[0]["id"] == "entry3"

    # Filtering by properties should work as well
    results = query_index(space_path, {"status": "todo"}, fs=fs)
    assert len(results) == EXPECTED_SINGLE_RESULT
    assert results[0]["id"] == "entry3"


def test_query_index_by_tag(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Filter cached index by tag membership."""
    fs, root = fs_impl
    space_path = fs_join(root, "spaces", "test_space")

    def fake_run_async(
        func: object,
        config: dict[str, str],
        space_id: str,
        payload: str,
    ) -> list[dict[str, object]]:
        assert func is ugoite_core.query_index
        assert space_id == "test_space"
        assert "uri" in config
        filters = json.loads(payload)
        if filters == {"tag": "project-alpha"}:
            return [
                {"id": "entry1", "tags": ["project-alpha", "urgent"]},
                {"id": "entry3", "tags": ["project-alpha"]},
            ]
        if filters == {"tag": "urgent"}:
            return [{"id": "entry1", "tags": ["project-alpha", "urgent"]}]
        return []

    monkeypatch.setattr(indexer_module, "run_async", fake_run_async)

    results = query_index(space_path, {"tag": "project-alpha"}, fs=fs)
    ids = {entry["id"] for entry in results}
    assert ids == {"entry1", "entry3"}

    results = query_index(space_path, {"tag": "urgent"}, fs=fs)
    assert len(results) == EXPECTED_SINGLE_RESULT
    assert results[0]["id"] == "entry1"


def test_indexer_watch_loop_triggers_run(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Ensure watch loop invokes run_once for each file-system event."""
    fs, root = fs_impl
    space_path = fs_join(root, "test_space")
    indexer = Indexer(space_path, fs=fs)

    calls: list[int] = []

    def fake_run_once() -> None:
        calls.append(1)

    monkeypatch.setattr(indexer, "run_once", fake_run_once)

    def wait_for_changes(callback: Callable[[], None]) -> None:
        callback()
        callback()

    indexer.watch(wait_for_changes)

    assert len(calls) == EXPECTED_WATCH_CALLBACKS


def test_indexer_computes_word_count() -> None:
    """Ensure word_count is computed for indexed entries."""
    content = "# Hello World\n\nThis is a test entry with some words."
    # "Hello World" (2) + "This is a test entry with some words." (8) = 10 words
    # Entry: split() counts '#' as a word, so 11.
    assert compute_word_count(content) == EXPECTED_WORD_COUNT


def test_aggregate_stats_includes_field_usage() -> None:
    """Ensure stats include field usage frequencies per form."""
    index_data = {
        "entry1": {
            "form": "meeting",
            "properties": {"Date": "2025-10-27", "Attendees": "Alice"},
        },
        "entry2": {
            "form": "meeting",
            "properties": {"Date": "2025-10-28"},  # Missing Attendees
        },
        "entry3": {
            "form": "task",
            "properties": {"status": "todo"},
        },
    }

    stats = aggregate_stats(index_data)

    # Check field usage for 'meeting'
    meeting_stats = stats["form_stats"]["meeting"]
    assert "fields" in meeting_stats
    assert meeting_stats["fields"]["Date"] == EXPECTED_FIELD_COUNT_2
    assert meeting_stats["fields"]["Attendees"] == EXPECTED_SINGLE_RESULT
    task_stats = stats["form_stats"]["task"]
    assert task_stats["fields"]["status"] == EXPECTED_SINGLE_RESULT


def test_indexer_generates_inverted_index(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
) -> None:
    """Ensure inverted index contains term-to-entry mappings."""
    _fs, _root = fs_impl
    inverted_index = build_inverted_index(
        {
            "entry1": {
                "id": "entry1",
                "title": "Meeting Entries",
                "form": "meeting",
                "tags": ["project", "alpha"],
                "properties": {"Agenda": "Discuss project alpha."},
            },
            "entry2": {
                "id": "entry2",
                "title": "Task List",
                "form": "task",
                "tags": ["priority"],
                "properties": {"Priority": "High priority task."},
            },
        },
    )

    # Check that terms map to the correct entries
    assert "meeting" in inverted_index
    assert "entry1" in inverted_index["meeting"]
    assert "entry2" not in inverted_index["meeting"]

    assert "task" in inverted_index
    assert "entry2" in inverted_index["task"]

    assert "project" in inverted_index
    assert "entry1" in inverted_index["project"]

    assert "alpha" in inverted_index
    assert "entry1" in inverted_index["alpha"]

    assert "priority" in inverted_index
    assert "entry2" in inverted_index["priority"]

    # Verify terms from properties
    assert "agenda" in inverted_index
    assert "entry1" in inverted_index["agenda"]
