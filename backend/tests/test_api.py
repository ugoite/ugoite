"""API tests."""

import base64
import hashlib
import hmac
import io
import json
from collections.abc import AsyncIterator
from pathlib import Path

import pytest
import ugoite_core
from fastapi.testclient import TestClient
from starlette.responses import StreamingResponse

from app.main import app


def _create_form(
    client: TestClient,
    space_id: str,
    form_name: str = "Entry",
    fields: dict[str, dict[str, object]] | None = None,
) -> None:
    resolved_fields = fields or {"Body": {"type": "markdown"}}
    form_def = {
        "name": form_name,
        "version": 1,
        "template": f"# {form_name}\n\n## Body\n",
        "fields": resolved_fields,
    }
    res = client.post(f"/spaces/{space_id}/forms", json=form_def)
    assert res.status_code == 201


def test_create_space(test_client: TestClient, temp_space_root: Path) -> None:
    """Test creating a new space."""
    response = test_client.post("/spaces", json={"name": "test-ws"})
    assert response.status_code == 201
    data = response.json()
    assert data["id"] == "test-ws"
    assert data["name"] == "test-ws"

    # Verify file system
    ws_path = temp_space_root / "spaces" / "test-ws"
    assert ws_path.exists()
    assert (ws_path / "meta.json").exists()


def test_health_endpoint(test_client: TestClient) -> None:
    """REQ-OPS-001: health endpoint returns service readiness."""
    response = test_client.get("/health")
    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


def test_create_space_rejects_invalid_name(test_client: TestClient) -> None:
    """REQ-API-001: create space rejects names violating identifier rules."""
    response = test_client.post("/spaces", json={"name": "invalid space"})
    assert response.status_code == 400
    assert "Invalid space_id" in response.json()["detail"]


def test_create_space_rejects_excessive_name_length(test_client: TestClient) -> None:
    """REQ-API-001: create space payload enforces max length constraints."""
    response = test_client.post("/spaces", json={"name": "a" * 129})
    assert response.status_code == 422


def test_create_space_conflict(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test creating a space that already exists."""
    # Create first time
    test_client.post("/spaces", json={"name": "test-ws"})

    # Create second time
    response = test_client.post("/spaces", json={"name": "test-ws"})
    assert response.status_code == 409
    assert "already exists" in response.json()["detail"]


def test_list_spaces(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test listing spaces."""
    # Create some spaces
    test_client.post("/spaces", json={"name": "ws1"})
    test_client.post("/spaces", json={"name": "ws2"})

    response = test_client.get("/spaces")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2


def test_list_spaces_redacts_hmac_key(test_client: TestClient) -> None:
    """REQ-API-001: /spaces MUST NOT expose hmac_key secrets."""
    test_client.post("/spaces", json={"name": "secret-space"})

    list_response = test_client.get("/spaces")
    assert list_response.status_code == 200
    spaces = list_response.json()
    target = next(space for space in spaces if space["id"] == "secret-space")
    assert "hmac_key" not in target

    detail_response = test_client.get("/spaces/secret-space")
    assert detail_response.status_code == 200
    detail = detail_response.json()
    assert "hmac_key" not in detail


def test_list_spaces_missing_root_creates_default(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """REQ-STO-009: /spaces succeeds with missing root path."""
    root = tmp_path / "missing-root"
    monkeypatch.setenv("UGOITE_ROOT", str(root))
    monkeypatch.setenv("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true")

    with TestClient(
        app,
        headers={"Authorization": "Bearer test-suite-token"},
    ) as client:
        response = client.get("/spaces")

    assert response.status_code == 200
    data = response.json()
    assert any(ws["id"] == "default" for ws in data)
    assert root.exists()


def test_list_spaces_missing_root_does_not_bootstrap_by_default(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """REQ-STO-009: startup stays read-only unless bootstrap is enabled."""
    root = tmp_path / "missing-root-no-bootstrap"
    monkeypatch.setenv("UGOITE_ROOT", str(root))
    monkeypatch.delenv("UGOITE_BOOTSTRAP_DEFAULT_SPACE", raising=False)

    with TestClient(
        app,
        headers={"Authorization": "Bearer test-suite-token"},
    ) as client:
        response = client.get("/spaces")

    assert response.status_code == 200
    data = response.json()
    assert all(ws["id"] != "default" for ws in data)


def test_list_spaces_handles_core_failure(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-009: /spaces returns explicit error on core failure."""

    async def _raise(_config: dict[str, str]) -> list[str]:
        msg = "boom"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "list_spaces", _raise)

    response = test_client.get("/spaces")
    assert response.status_code == 500
    assert response.json()["detail"] == "Failed to list spaces"


def test_list_spaces_fails_when_space_meta_read_errors(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-009: /spaces fails explicitly when per-space metadata read fails."""

    async def _list_spaces(_config: dict[str, str]) -> list[str]:
        return ["ws-a"]

    async def _allow(*_args: object, **_kwargs: object) -> None:
        return None

    async def _raise_get_space(*_args: object, **_kwargs: object) -> dict[str, object]:
        msg = "meta read failed"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "list_spaces", _list_spaces)
    monkeypatch.setattr(ugoite_core, "require_space_action", _allow)
    monkeypatch.setattr(ugoite_core, "get_space", _raise_get_space)

    response = test_client.get("/spaces")
    assert response.status_code == 500
    assert response.json()["detail"] == "Failed to read space metadata"


def test_get_space(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting a specific space."""
    test_client.post("/spaces", json={"name": "test-ws"})

    response = test_client.get("/spaces/test-ws")
    assert response.status_code == 200
    data = response.json()
    assert data["id"] == "test-ws"


def test_get_space_not_found(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting a non-existent space."""
    response = test_client.get("/spaces/nonexistent")
    assert response.status_code == 404


def test_create_entry(test_client: TestClient, temp_space_root: Path) -> None:
    """Test creating a entry in a space."""
    # Create space first
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")

    entry_payload = {
        "content": "---\nform: Entry\n---\n# My Entry\n\n## Body\nSome content",
    }

    response = test_client.post("/spaces/test-ws/entries", json=entry_payload)
    assert response.status_code == 201
    data = response.json()
    assert "id" in data
    assert "revision_id" in data  # Required for optimistic concurrency
    entry_id = data["id"]

    # Verify retrieval
    get_response = test_client.get(f"/spaces/test-ws/entries/{entry_id}")
    assert get_response.status_code == 200


def test_create_entry_conflict(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test creating a entry with an existing ID (if ID is provided)."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")

    # Create entry with specific ID
    entry_id = "my-entry"
    entry_payload = {
        "id": entry_id,
        "content": "---\nform: Entry\n---\n# My Entry\n\n## Body\n",
    }

    test_client.post("/spaces/test-ws/entries", json=entry_payload)

    # Try again
    response = test_client.post("/spaces/test-ws/entries", json=entry_payload)
    assert response.status_code == 409


def test_create_entry_rejects_invalid_client_id(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-ENTRY-001: entry create rejects invalid client-provided ids."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")

    response = test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "bad id",
            "content": "---\nform: Entry\n---\n# Title\n\n## Body\ntext",
        },
    )
    assert response.status_code == 400
    assert "Invalid entry_id" in response.json()["detail"]


def test_create_entry_rejects_empty_client_id(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-ENTRY-001: entry create rejects empty-but-present client ids."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")

    response = test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "",
            "content": "---\nform: Entry\n---\n# Title\n\n## Body\ntext",
        },
    )
    assert response.status_code == 400
    assert "Invalid entry_id" in response.json()["detail"]


def test_list_entries(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test listing entries in a space."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry1",
            "content": """---
form: Entry
---
# Entry 1

## Body
One""",
        },
    )
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry2",
            "content": """---
form: Entry
---
# Entry 2

## Body
Two""",
        },
    )

    response = test_client.get("/spaces/test-ws/entries")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2

    # Verify EntryRecord structure includes properties and links
    for entry in data:
        assert "id" in entry
        assert "title" in entry
        assert "properties" in entry, (
            "properties field must be present in entry list response"
        )
        assert "links" in entry, "links field must be present in entry list response"
        assert isinstance(entry["properties"], dict)
        assert isinstance(entry["links"], list)


def test_list_entries_space_not_found_returns_404(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """GET /spaces/{id}/entries should return 404 when space root.

    lacks spaces dir.
    """
    # temp_space_root fixture sets UGOITE_ROOT to an empty temporary directory
    response = test_client.get("/spaces/Stay/entries")
    assert response.status_code == 404


def test_get_entry(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting a specific entry."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# Test Entry\n\n## Body\nContent here",
        },
    )

    response = test_client.get("/spaces/test-ws/entries/test-entry")
    assert response.status_code == 200
    data = response.json()
    assert data["id"] == "test-entry"
    assert data["title"] == "Test Entry"
    # Entry: get_entry returns "content" field (not "markdown")
    assert "# Test Entry" in data["content"]


def test_get_entry_not_found(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting a non-existent entry."""
    test_client.post("/spaces", json={"name": "test-ws"})

    response = test_client.get("/spaces/test-ws/entries/nonexistent")
    assert response.status_code == 404


def test_update_entry(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test updating a entry."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": """---
form: Entry
---
# Original Title

## Body
Original body""",
        },
    )

    # Get the entry to get the revision_id
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    revision_id = get_response.json()["revision_id"]

    # Update the entry
    update_payload = {
        "markdown": "---\nform: Entry\n---\n# Updated Title\n\n## Body\nNew content",
        "parent_revision_id": revision_id,
    }

    response = test_client.put(
        "/spaces/test-ws/entries/test-entry",
        json=update_payload,
    )
    assert response.status_code == 200
    data = response.json()
    assert "id" in data
    assert "revision_id" in data
    assert data["revision_id"] != revision_id  # New revision

    # Verify the entry was updated by fetching it
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    updated_entry = get_response.json()
    assert updated_entry["title"] == "Updated Title"
    # Entry: get_entry returns "content" field (not "markdown")
    assert "New content" in updated_entry["content"]


def test_update_entry_form_validation_error_returns_422_and_does_not_update(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Updating a formed entry should fail with 422 when it violates the form."""
    test_client.post("/spaces", json={"name": "test-ws"})

    form_def = {
        "name": "Meeting",
        "version": 1,
        "template": "# Meeting\n\n## Date\n",
        "fields": {"Date": {"type": "date", "required": True}},
        "defaults": None,
    }
    res = test_client.post("/spaces/test-ws/forms", json=form_def)
    assert res.status_code == 201

    # Create a entry with form Meeting and required Date property
    entry_content = """---
form: Meeting
---
# Meeting entries

## Date
2025-01-01
"""
    res = test_client.post(
        "/spaces/test-ws/entries",
        json={"id": "meeting-1", "content": entry_content},
    )
    assert res.status_code == 201

    get_res = test_client.get("/spaces/test-ws/entries/meeting-1")
    assert get_res.status_code == 200
    original = get_res.json()
    original_revision_id = original["revision_id"]

    update_res = test_client.put(
        "/spaces/test-ws/entries/meeting-1",
        json={
            "markdown": """---
form: Meeting
---
# Meeting entries

## Date
2025-01-01

## Extra
Nope
""",
            "parent_revision_id": original_revision_id,
        },
    )
    assert update_res.status_code == 422
    assert "Unknown form fields" in update_res.json()["detail"]

    # Ensure it did not update the revision
    get_res = test_client.get("/spaces/test-ws/entries/meeting-1")
    assert get_res.status_code == 200
    after = get_res.json()
    assert after["revision_id"] == original_revision_id


def test_update_entry_conflict(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test updating a entry with a stale parent_revision_id returns 409."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get the original revision_id
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    original_revision_id = get_response.json()["revision_id"]

    # First update succeeds
    test_client.put(
        "/spaces/test-ws/entries/test-entry",
        json={
            "markdown": "---\nform: Entry\n---\n# Update 1\n\n## Body\nUpdate one",
            "parent_revision_id": original_revision_id,
        },
    )

    # Second update with stale revision_id should fail with 409
    response = test_client.put(
        "/spaces/test-ws/entries/test-entry",
        json={
            "markdown": "---\nform: Entry\n---\n# Update 2\n\n## Body\nUpdate two",
            "parent_revision_id": original_revision_id,  # Stale!
        },
    )
    assert response.status_code == 409
    # Should include the current revision for client merge
    detail = response.json()["detail"]
    conflict_check = "conflict" in str(detail).lower() or "current_revision" in str(
        detail,
    )
    assert conflict_check


def test_delete_entry(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test deleting (tombstoning) a entry."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# To Delete\n\n## Body\nDelete me",
        },
    )

    response = test_client.delete("/spaces/test-ws/entries/test-entry")
    assert response.status_code == 200
    data = response.json()
    assert data["status"] == "deleted"

    # Deleted entries should not appear in list
    list_response = test_client.get("/spaces/test-ws/entries")
    entries = list_response.json()
    entry_ids = [n["id"] for n in entries]
    assert "test-entry" not in entry_ids


def test_get_entry_history(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting entry history."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get initial revision
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    revision_id = get_response.json()["revision_id"]

    # Update to create another revision
    test_client.put(
        "/spaces/test-ws/entries/test-entry",
        json={
            "markdown": "---\nform: Entry\n---\n# Updated\n\n## Body\nUpdated",
            "parent_revision_id": revision_id,
        },
    )

    # Get history
    response = test_client.get("/spaces/test-ws/entries/test-entry/history")
    assert response.status_code == 200
    data = response.json()
    assert "revisions" in data
    assert len(data["revisions"]) == 2


def test_get_entry_revision(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test getting a specific revision."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get the revision_id
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    revision_id = get_response.json()["revision_id"]

    # Get the specific revision
    response = test_client.get(
        f"/spaces/test-ws/entries/test-entry/history/{revision_id}",
    )
    assert response.status_code == 200
    data = response.json()
    assert data["revision_id"] == revision_id


def test_restore_entry(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test restoring a entry to a previous revision."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "test-entry",
            "content": "---\nform: Entry\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get original revision
    get_response = test_client.get("/spaces/test-ws/entries/test-entry")
    original_revision_id = get_response.json()["revision_id"]

    # Update the entry
    test_client.put(
        "/spaces/test-ws/entries/test-entry",
        json={
            "markdown": "---\nform: Entry\n---\n# Updated\n\n## Body\nUpdated",
            "parent_revision_id": original_revision_id,
        },
    )

    # Restore to original
    response = test_client.post(
        "/spaces/test-ws/entries/test-entry/restore",
        json={"revision_id": original_revision_id},
    )
    assert response.status_code == 200
    data = response.json()
    assert "revision_id" in data
    assert data["restored_from"] == original_revision_id


def test_query_entries(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test structured query endpoint."""
    test_client.post("/spaces", json={"name": "test-ws"})

    # Create a entry that should be indexed
    # Entry: In a real scenario, the indexer runs in background.
    # For this test, we might need to mock the index or manually update it
    # if the API reads from index.json
    # However, the Milestone 2 implementation of `ugoite.query` should read
    # from index.json.
    # Since we haven't implemented the background indexer in the backend yet,
    # we might need to manually populate index.json or rely on the API to
    # trigger indexing (unlikely per spec).
    # Or, we just test that the endpoint exists and returns empty list for now.

    response = test_client.post("/spaces/test-ws/query", json={"filter": {}})
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_query_entries_rejects_oversized_filter(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-STO-004: Query endpoint rejects oversized filter payloads."""
    test_client.post("/spaces", json={"name": "test-ws"})
    oversized_value = "x" * 40_000
    response = test_client.post(
        "/spaces/test-ws/query",
        json={"filter": {"note": oversized_value}},
    )
    assert response.status_code == 400
    assert "Query filter too large" in response.json()["detail"]


def test_query_entries_sql(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-API-008: SQL session queries should return matching entries."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")

    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-sql-1",
            "content": "---\nform: Entry\n---\n# Alpha\n\n## Body\nalpha topic",
        },
    )
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-sql-2",
            "content": "---\nform: Entry\n---\n# Beta\n\n## Body\nbeta topic",
        },
    )

    sql_query = "SELECT * FROM entries WHERE title = 'Alpha'"
    sql_res = test_client.post(
        "/spaces/test-ws/sql",
        json={
            "id": "sql-alpha",
            "name": "Alpha Query",
            "sql": sql_query,
            "variables": [],
        },
    )
    assert sql_res.status_code == 201

    response = test_client.post(
        "/spaces/test-ws/sql-sessions",
        json={"sql": sql_query},
    )
    assert response.status_code == 201
    session = response.json()
    assert session["status"] == "ready"
    session_id = session["id"]

    count_res = test_client.get(
        f"/spaces/test-ws/sql-sessions/{session_id}/count",
    )
    assert count_res.status_code == 200
    assert count_res.json()["count"] == 1

    rows_res = test_client.get(
        f"/spaces/test-ws/sql-sessions/{session_id}/rows",
        params={"offset": 0, "limit": 50},
    )
    assert rows_res.status_code == 200
    rows_payload = rows_res.json()
    assert rows_payload["total_count"] == 1
    rows = rows_payload["rows"]
    assert len(rows) == 1
    assert rows[0]["id"] == "entry-sql-1"


def test_sql_session_stream_uses_incremental_row_paging(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-008: sql session stream retrieves rows incrementally."""
    test_client.post("/spaces", json={"name": "test-ws"})

    call_offsets: list[int] = []

    async def _paged_rows(
        _config: dict[str, str],
        _space_id: str,
        _session_id: str,
        offset: int,
        _limit: int,
    ) -> dict[str, object]:
        call_offsets.append(offset)
        if offset == 0:
            return {
                "rows": [
                    {"id": "entry-1", "title": "Alpha"},
                    {"id": "entry-2", "title": "Beta"},
                ],
                "total_count": 2,
            }
        return {"rows": [], "total_count": 2}

    async def _rows_all(*_args: object, **_kwargs: object) -> list[dict[str, object]]:
        msg = "rows_all must not be called"
        raise AssertionError(msg)

    monkeypatch.setattr(ugoite_core, "get_sql_session_rows", _paged_rows)
    monkeypatch.setattr(ugoite_core, "get_sql_session_rows_all", _rows_all)

    response = test_client.get("/spaces/test-ws/sql-sessions/session-1/stream")
    assert response.status_code == 200
    assert response.headers["content-type"].startswith("application/x-ndjson")
    assert call_offsets == [0]
    lines = [line for line in response.text.splitlines() if line.strip()]
    assert len(lines) == 2
    assert json.loads(lines[0])["id"] == "entry-1"


def test_create_sql_session_validation_error_returns_422(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-008: sql session creation maps SQL validation errors to 422."""
    test_client.post("/spaces", json={"name": "test-ws"})

    async def _raise(
        _config: dict[str, str],
        _space_id: str,
        _sql: str,
    ) -> dict[str, object]:
        msg = "UGOITE_SQL_VALIDATION: invalid sql"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "create_sql_session", _raise)

    response = test_client.post(
        "/spaces/test-ws/sql-sessions",
        json={"sql": "SELECT FROM"},
    )

    assert response.status_code == 422
    assert "UGOITE_SQL_VALIDATION" in response.json()["detail"]


def test_upload_asset_and_link_to_entry(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Assets can be uploaded, returned with id, and linked to a entry."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    entry_res = test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-1",
            "content": "---\nform: Entry\n---\n# Attach Entry\n\n## Body\nAttach",
        },
    )
    assert entry_res.status_code == 201

    file_bytes = b"hello asset"
    response = test_client.post(
        "/spaces/test-ws/assets",
        files={"file": ("voice.m4a", io.BytesIO(file_bytes), "audio/m4a")},
    )
    assert response.status_code == 201
    asset = response.json()
    assert asset["id"]
    assert asset["path"].startswith("assets/")

    update_res = test_client.put(
        "/spaces/test-ws/entries/entry-1",
        json={
            "markdown": "---\nform: Entry\n---\n# Attach Entry\n\n## Body\ncontent",
            "parent_revision_id": entry_res.json()["revision_id"],
            "assets": [asset],
        },
    )
    assert update_res.status_code == 200

    # Ensure GET reflects the asset reference
    get_res = test_client.get("/spaces/test-ws/entries/entry-1")
    assert get_res.status_code == 200
    content = get_res.json()
    assert any(a["id"] == asset["id"] for a in content.get("assets", []))


def test_delete_asset_referenced_fails(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Deleting an asset referenced by a entry should fail."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    entry_res = test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-1",
            "content": "---\nform: Entry\n---\n# Attach Entry\n\n## Body\nAttach",
        },
    )

    response = test_client.post(
        "/spaces/test-ws/assets",
        files={"file": ("voice.m4a", io.BytesIO(b"data"), "audio/m4a")},
    )
    asset = response.json()

    test_client.put(
        "/spaces/test-ws/entries/entry-1",
        json={
            "markdown": "---\nform: Entry\n---\n# Attach Entry\n\n## Body\nupdated",
            "parent_revision_id": entry_res.json()["revision_id"],
            "assets": [asset],
        },
    )

    delete_res = test_client.delete(
        f"/spaces/test-ws/assets/{asset['id']}",
    )
    assert delete_res.status_code == 409
    assert "referenced" in delete_res.json()["detail"].lower()


def test_delete_asset_not_found_has_consistent_detail(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-API-001: Delete asset 404 detail includes resource context."""
    test_client.post("/spaces", json={"name": "test-ws"})
    delete_res = test_client.delete("/spaces/test-ws/assets/missing-asset")
    assert delete_res.status_code == 404
    detail = delete_res.json()["detail"]
    assert "missing-asset" in detail
    assert "test-ws" in detail


def test_search_returns_matches(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Hybrid search returns entries containing the keyword via inverted index."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "alpha",
            "content": "---\nform: Entry\n---\n# Alpha\n\n## Body\nProject rocket",
        },
    )

    search_res = test_client.get("/spaces/test-ws/search", params={"q": "rocket"})
    assert search_res.status_code == 200
    ids = [n["id"] for n in search_res.json()]
    assert "alpha" in ids


def test_search_rejects_oversized_query(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-STO-004: Search rejects oversized q payloads with explicit 400 error."""
    test_client.post("/spaces", json={"name": "test-ws"})
    oversized = "q" * 513
    search_res = test_client.get("/spaces/test-ws/search", params={"q": oversized})
    assert search_res.status_code == 400
    assert "Query too long" in search_res.json()["detail"]


def test_update_space_storage_connector(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """PATCH space should persist storage connector details."""
    test_client.post("/spaces", json={"name": "test-ws"})

    patch_res = test_client.patch(
        "/spaces/test-ws",
        json={
            "storage_config": {
                "uri": "s3://bucket/path",
                "credentials_profile": "default",
            },
            "settings": {"default_form": "Meeting"},
        },
    )
    assert patch_res.status_code == 200
    data = patch_res.json()
    assert data["storage_config"]["uri"] == "s3://bucket/path"
    assert data["settings"]["default_form"] == "Meeting"


def test_test_connection_endpoint(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """POST /test-connection returns success for local connector stub."""
    test_client.post("/spaces", json={"name": "test-ws"})
    res = test_client.post(
        "/spaces/test-ws/test-connection",
        json={"storage_config": {"uri": "file:///tmp"}},
    )
    assert res.status_code == 200
    payload = res.json()
    assert payload["status"] == "ok"


def test_middleware_headers(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test that security headers are present."""
    response = test_client.get("/")
    assert "X-Content-Type-Options" in response.headers
    assert response.headers["X-Content-Type-Options"] == "nosniff"


def test_middleware_hmac_signature(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test that HMAC signature header matches the response body."""
    response = test_client.get("/")

    global_data = json.loads((temp_space_root / "hmac.json").read_text())
    secret = base64.b64decode(global_data["hmac_key"])
    expected_signature = hmac.new(
        secret,
        response.content,
        hashlib.sha256,
    ).hexdigest()

    assert response.headers["X-Ugoite-Key-Id"] == global_data["hmac_key_id"]
    assert response.headers["X-Ugoite-Signature"] == expected_signature


def test_middleware_signs_non_streaming_mcp_prefixed_paths(
    test_client: TestClient,
) -> None:
    """REQ-INT-003: non-streaming /mcp* paths MUST still be response-signed."""
    if not hasattr(app.state, "mcp_test_route_registered"):

        @app.get("/mcp-test-json")
        async def _mcp_test_json() -> dict[str, str]:
            return {"status": "ok"}

        app.state.mcp_test_route_registered = True

    response = test_client.get("/mcp-test-json")
    assert response.status_code == 200
    assert "X-Ugoite-Signature" in response.headers


def test_middleware_replays_chunked_body_without_loss(test_client: TestClient) -> None:
    """REQ-INT-003: middleware preserves chunked response body while signing."""
    if not hasattr(app.state, "chunked_route_registered"):

        @app.get("/chunked-test-body")
        async def _chunked_test_body() -> StreamingResponse:
            async def _iter() -> AsyncIterator[bytes]:
                for chunk in (b"a", b"b", b"c"):
                    yield chunk

            return StreamingResponse(_iter(), media_type="text/plain")

        app.state.chunked_route_registered = True

    response = test_client.get("/chunked-test-body")
    assert response.status_code == 200
    assert response.content == b"abc"
    assert "X-Ugoite-Signature" in response.headers


def test_middleware_ignores_untrusted_forwarded_for(
    test_client: TestClient,
) -> None:
    """REQ-SEC-001: ignore spoofed forwarded headers in untrusted mode."""
    response = test_client.get("/", headers={"x-forwarded-for": "203.0.113.10"})
    assert response.status_code == 200
    assert "X-Ugoite-Signature" in response.headers


def test_middleware_blocks_remote_clients_when_proxy_headers_trusted(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-001: trusted proxy mode enforces forwarded remote blocking."""
    monkeypatch.setenv("UGOITE_TRUST_PROXY_HEADERS", "true")
    response = test_client.get("/", headers={"x-forwarded-for": "203.0.113.10"})
    assert response.status_code == 403
    assert "Remote access is disabled" in response.json()["detail"]
    assert "X-Ugoite-Signature" in response.headers


def test_get_form_types(test_client: TestClient, temp_space_root: Path) -> None:
    """Test getting available form column types (REQ-FORM-001)."""
    # Create space to ensure path is valid
    test_client.post("/spaces", json={"name": "test-ws-types"})

    response = test_client.get("/spaces/test-ws-types/forms/types")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert "string" in data
    assert "number" in data
    assert "row_reference" in data


def test_update_form_with_migration(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test updating form with migration strategies (REQ-FORM-002)."""
    # 1. Create Space
    test_client.post("/spaces", json={"name": "test-ws-mig"})

    # 2. Create Initial Form
    entry_form = {
        "name": "project",
        "template": "# Project",
        "fields": {
            "status": {"type": "string"},
        },
    }
    test_client.post("/spaces/test-ws-mig/forms", json=entry_form)

    # 3. Create Entry
    entry_payload = {
        "content": "---\nform: project\n---\n# Project A\n\n## status\nActive\n",
    }
    # Using endpoints: POST /spaces/{id}/entries
    # It autogenerates ID.
    res = test_client.post("/spaces/test-ws-mig/entries", json=entry_payload)
    assert res.status_code == 201
    entry_id = res.json()["id"]

    # 4. Update Form with new field and migration
    updated_form = {
        "name": "project",
        "template": "# Project",
        "fields": {
            "status": {"type": "string"},
            "priority": {"type": "string"},
        },
        "strategies": {
            "priority": "High",
        },
    }
    res = test_client.post("/spaces/test-ws-mig/forms", json=updated_form)
    assert res.status_code == 201

    # 5. Verify Entry
    res = test_client.get(f"/spaces/test-ws-mig/entries/{entry_id}")
    assert res.status_code == 200
    entry_data = res.json()
    content = entry_data["content"]
    assert "## priority" in content
    assert "High" in content


import asyncio
import json as _json
from collections.abc import AsyncGenerator
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch
from fastapi import HTTPException
from starlette.responses import Response
from app.api.endpoints.search import _is_sql_error
from app.api.endpoints.space import (
    _sanitize_space_meta,
    _validate_entry_markdown_against_form,
)
from app.core.middleware import (
    _AuditRequestEvent,
    _capture_response_body,
    _emit_audit_event,
    security_middleware,
)
from app.core.storage import _ensure_local_root, storage_config_from_root
from app.mcp.server import _context_headers, list_entries


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_ensure_local_root_file_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root handles file:// URIs."""
    target = tmp_path / "new_dir"
    _ensure_local_root(f"file://{target}")
    assert target.exists()


def test_ensure_local_root_fs_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root handles fs:// URIs."""
    target = tmp_path / "fs_dir"
    _ensure_local_root(f"fs://{target}")
    assert target.exists()


def test_ensure_local_root_oserror_plain_path(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root propagates OSError for plain paths."""
    with (
        patch("pathlib.Path.mkdir", side_effect=OSError("Permission denied")),
        pytest.raises(OSError, match="Permission denied"),
    ):
        _ensure_local_root("/nonexistent/deeply/nested/path")


def test_ensure_local_root_oserror_file_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root propagates OSError for file:// paths."""
    with (
        patch("pathlib.Path.mkdir", side_effect=OSError("Permission denied")),
        pytest.raises(OSError, match="Permission denied"),
    ):
        _ensure_local_root(f"file://{tmp_path}/blocked")


def test_ensure_local_root_non_file_scheme() -> None:
    """REQ-STO-001: _ensure_local_root returns early for non-file schemes."""
    # Should not raise - s3:// scheme is not file-based, just returns
    _ensure_local_root("s3://my-bucket/spaces")


def test_ensure_local_root_file_scheme_empty_path() -> None:
    """REQ-STO-001: _ensure_local_root raises ValueError for empty local path."""
    # Mock Path so str() returns "" to trigger the empty path guard
    mock_path_instance = MagicMock()
    mock_path_instance.__str__ = MagicMock(return_value="")

    with (
        patch("app.core.storage.Path", return_value=mock_path_instance),
        pytest.raises(ValueError, match="Local storage path is empty"),
    ):
        _ensure_local_root("file:///some/path")


def test_capture_response_body_without_iterator() -> None:
    """REQ-SEC-002: _capture_response_body reads body attribute when no iterator."""
    response = Response(content=b"direct body content")
    result = asyncio.run(_capture_response_body(response))
    assert result == b"direct body content"


def test_middleware_sse_response_not_captured(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-001: SSE responses bypass body capture in security middleware."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.client.host = "127.0.0.1"
    mock_request.url.path = "/"
    mock_request.headers = {}

    async def _gen() -> AsyncGenerator[bytes]:
        yield b"data: test\n\n"

    sse_response = StreamingResponse(_gen(), media_type="text/event-stream")

    async def _call_next(_req: object) -> Response:
        return sse_response

    result = asyncio.run(security_middleware(mock_request, _call_next))
    assert result is sse_response


def test_middleware_403_non_json_body_handled(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-002: middleware handles non-JSON 403 response body gracefully."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.client.host = "127.0.0.1"
    mock_request.url.path = "/"
    mock_request.method = "GET"
    mock_request.headers = {}

    forbidden_response = Response(content=b"not valid json", status_code=403)

    async def _call_next(_req: object) -> Response:
        return forbidden_response

    async def _fake_sign(_body: bytes, _root: object) -> tuple[str, str]:
        return "kid", "sig"

    with patch("app.core.middleware.build_response_signature", _fake_sign):
        result = asyncio.run(security_middleware(mock_request, _call_next))
    # Should not raise; 403 with non-JSON body is handled
    assert result.status_code == 403


def test_middleware_emit_audit_runtime_error_swallowed(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-008: RuntimeError in audit emission is logged and swallowed."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.url.path = "/spaces/test-space/entries"
    mock_request.headers = {}
    mock_request.method = "GET"

    event = _AuditRequestEvent(
        action="data.mutation",
        outcome="success",
        actor_user_id="user1",
    )

    with patch(
        "ugoite_core.append_audit_event",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        # Should not raise; RuntimeError is caught and logged
        asyncio.run(_emit_audit_event(mock_request, event))


def test_context_headers_request_none_raises() -> None:
    """REQ-API-001: _context_headers raises when request context is None."""
    ctx = MagicMock()
    ctx.request_context.request = None
    with pytest.raises(RuntimeError, match="Missing authentication context"):
        _context_headers(ctx)


def test_context_headers_headers_none_raises() -> None:
    """REQ-API-001: _context_headers raises when headers cannot be resolved."""
    ctx = MagicMock()
    # request has no headers attribute (enforced by the spec) and is not a dict
    request = MagicMock(spec=["method", "url", "path"])
    ctx.request_context.request = request
    with pytest.raises(RuntimeError, match="Missing request headers"):
        _context_headers(ctx)


def test_context_headers_dict_request() -> None:
    """REQ-API-001: _context_headers resolves headers from dict-style request."""
    ctx = MagicMock()
    ctx.request_context.request = {
        "headers": {"authorization": "Bearer token"},
        "method": "GET",
        "path": "/spaces/s/entries",
    }
    headers, _, _, _ = _context_headers(ctx)
    assert headers == {"authorization": "Bearer token"}


def test_context_headers_with_url_object() -> None:
    """REQ-API-001: _context_headers extracts path from request.url.path."""
    ctx = MagicMock()
    request = MagicMock()
    request.headers = {"authorization": "Bearer token", "x-request-id": "req-123"}
    request.url.path = "/spaces/test/entries"
    request.method = "GET"
    ctx.request_context.request = request
    _, _, path, req_id = _context_headers(ctx)
    assert path == "/spaces/test/entries"
    assert req_id == "req-123"


def test_list_entries_mcp(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-002: MCP list_entries resource returns JSON-encoded entries."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    ctx = MagicMock()
    request = MagicMock()
    request.headers = {"authorization": "Bearer test-token"}
    request.url.path = "/spaces/mcp-space/entries"
    request.method = "GET"
    ctx.request_context.request = request

    fake_identity = MagicMock()
    fake_entries = [{"id": "e1", "content": "# Hello"}]

    async def _run() -> str:
        with (
            patch(
                "app.mcp.server.authenticate_headers_for_space",
                _amock(return_value=fake_identity),
            ),
            patch(
                "ugoite_core.require_space_action",
                _amock(return_value=None),
            ),
            patch(
                "ugoite_core.list_entries",
                _amock(return_value=fake_entries),
            ),
            patch(
                "ugoite_core.filter_readable_entries",
                _amock(return_value=fake_entries),
            ),
        ):
            return await list_entries("mcp-space", ctx)

    result = asyncio.run(_run())
    assert _json.loads(result) == fake_entries


def test_list_assets_success(test_client: TestClient) -> None:
    """REQ-API-001: list assets returns empty list for new space."""
    test_client.post("/spaces", json={"name": "asset-list-ws"})
    response = test_client.get("/spaces/asset-list-ws/assets")
    assert response.status_code == 200
    assert response.json() == []


def test_list_assets_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list assets returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/asset-authz-ws/assets")
    assert response.status_code == 403


def test_list_assets_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: list assets returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "asset-err-ws"})
    with patch(
        "ugoite_core.list_assets",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.get("/spaces/asset-err-ws/assets")
    assert response.status_code == 500


def test_upload_asset_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: upload asset returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-upload-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_write",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/asset-upload-authz-ws/assets",
            files={"file": ("test.txt", b"hello", "text/plain")},
        )
    assert response.status_code == 403


def test_upload_asset_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: upload asset returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "asset-upload-exc-ws"})
    with patch(
        "ugoite_core.save_asset",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/asset-upload-exc-ws/assets",
            files={"file": ("test.txt", b"hello", "text/plain")},
        )
    assert response.status_code == 500


def test_delete_asset_success(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 200 when asset is deleted successfully."""
    test_client.post("/spaces", json={"name": "asset-del-ws"})
    upload_response = test_client.post(
        "/spaces/asset-del-ws/assets",
        files={"file": ("test.txt", b"hello asset", "text/plain")},
    )
    assert upload_response.status_code == 201
    asset_id = upload_response.json()["id"]

    delete_response = test_client.delete(
        f"/spaces/asset-del-ws/assets/{asset_id}",
    )
    assert delete_response.status_code == 200
    assert delete_response.json()["status"] == "deleted"


def test_delete_asset_runtime_error_generic(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 500 for non-ref/non-notfound RuntimeError."""
    test_client.post("/spaces", json={"name": "asset-del-err-ws"})
    with patch(
        "ugoite_core.delete_asset",
        _amock(side_effect=RuntimeError("unexpected storage error")),
    ):
        response = test_client.delete(
            "/spaces/asset-del-err-ws/assets/some-asset",
        )
    assert response.status_code == 500


def test_delete_asset_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 500 for unexpected exception."""
    test_client.post("/spaces", json={"name": "asset-del-exc-ws"})
    with patch(
        "ugoite_core.delete_asset",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.delete(
            "/spaces/asset-del-exc-ws/assets/some-asset",
        )
    assert response.status_code == 500


def test_delete_asset_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: delete asset returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-del-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_write",
            ),
        ),
    ):
        response = test_client.delete(
            "/spaces/asset-del-authz-ws/assets/some-asset",
        )
    assert response.status_code == 403


def test_create_entry_already_exists(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 409 when entry already exists."""
    test_client.post("/spaces", json={"name": "entry-dup-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("entry already exists")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-dup-ws/entries",
            json={"id": "e-dup-2", "content": "# Title\n"},
        )
    assert response.status_code == 409


def test_create_entry_form_error(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 422 when form name is unknown."""
    test_client.post("/spaces", json={"name": "entry-form-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("unknown form type")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-form-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 422


def test_create_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-exc-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=ValueError("unexpected error")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-exc-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 500


def test_create_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-create-rt-ws"})
    with (
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-create-rt-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 500


def test_list_entries_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: list entries returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-list-exc-ws"})
    with patch(
        "ugoite_core.list_entries",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/entry-list-exc-ws/entries")
    assert response.status_code == 500


def test_get_entry_not_found_req_api_002(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-get-404-ws"})
    response = test_client.get("/spaces/entry-get-404-ws/entries/missing-entry")
    assert response.status_code == 404


def test_get_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-get-exc-ws"})
    with patch(
        "ugoite_core.get_entry",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/entry-get-exc-ws/entries/e1")
    assert response.status_code == 500


def test_get_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-get-rt-ws"})
    with (
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
    ):
        response = test_client.get("/spaces/entry-get-rt-ws/entries/e1")
    assert response.status_code == 500


def test_update_entry_conflict_then_404(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 409 on revision conflict."""
    test_client.post("/spaces", json={"name": "entry-upd-conflict-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("revision conflict detected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-conflict-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "old-rev"},
        )
    assert response.status_code == 409


def test_update_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-upd-404-ws"})
    with patch(
        "ugoite_core.get_entry",
        _amock(side_effect=RuntimeError("entry not found")),
    ):
        response = test_client.put(
            "/spaces/entry-upd-404-ws/entries/missing",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 404


def test_update_entry_form_validation_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 422 for form validation failure."""
    test_client.post("/spaces", json={"name": "entry-upd-form-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("unknown form reference")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-form-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 422


def test_update_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-upd-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("generic storage error")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-rt-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 500


def test_update_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 500 on unexpected non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-upd-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-exc-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 500


def test_update_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: update entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-upd-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-authz-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 403


def test_update_entry_conflict_retry_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry 409 when conflict retry also fails."""
    test_client.post("/spaces", json={"name": "entry-upd-retry-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    # First get_entry returns fake_entry, second raises RuntimeError
    with (
        patch(
            "ugoite_core.get_entry",
            AsyncMock(
                side_effect=[
                    fake_entry,
                    RuntimeError("storage error during retry"),
                ],
            ),
        ),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("revision conflict detected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-retry-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "old-rev"},
        )
    assert response.status_code == 409


def test_delete_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-del-404-ws"})
    response = test_client.delete("/spaces/entry-del-404-ws/entries/missing")
    assert response.status_code == 404


def test_delete_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-del-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.delete_entry",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-rt-ws/entries/e1")
    assert response.status_code == 500


def test_delete_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-del-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.delete_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-exc-ws/entries/e1")
    assert response.status_code == 500


def test_delete_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: delete entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-del-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-authz-ws/entries/e1")
    assert response.status_code == 403


def test_get_entry_history_not_found(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-hist-404-ws"})
    response = test_client.get(
        "/spaces/entry-hist-404-ws/entries/missing/history",
    )
    assert response.status_code == 404


def test_get_entry_history_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-hist-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_history",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-rt-ws/entries/e1/history",
        )
    assert response.status_code == 500


def test_get_entry_history_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-hist-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_history",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-exc-ws/entries/e1/history",
        )
    assert response.status_code == 500


def test_get_entry_history_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get entry history returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-hist-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_read",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_read",
                ),
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-authz-ws/entries/e1/history",
        )
    assert response.status_code == 403


def test_get_entry_revision_not_found(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 404 when revision does not exist."""
    test_client.post("/spaces", json={"name": "entry-rev-404-ws"})
    test_client.post(
        "/spaces/entry-rev-404-ws/entries",
        json={"id": "e1", "content": "# Title\n"},
    )
    response = test_client.get(
        "/spaces/entry-rev-404-ws/entries/e1/history/missing-rev",
    )
    assert response.status_code == 404


def test_get_entry_revision_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-rev-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_revision",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-rt-ws/entries/e1/history/some-rev",
        )
    assert response.status_code == 500


def test_get_entry_revision_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-rev-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_revision",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-exc-ws/entries/e1/history/some-rev",
        )
    assert response.status_code == 500


def test_get_entry_revision_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get entry revision returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-rev-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_read",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_read",
                ),
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-authz-ws/entries/e1/history/rev1",
        )
    assert response.status_code == 403


def test_restore_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-restore-404-ws"})
    response = test_client.post(
        "/spaces/entry-restore-404-ws/entries/missing/restore",
        json={"revision_id": "rev1"},
    )
    assert response.status_code == 404


def test_restore_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-restore-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.restore_entry",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-rt-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 500


def test_restore_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-restore-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.restore_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-exc-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 500


def test_restore_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: restore entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-restore-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-authz-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 403


def test_sanitize_space_meta_without_settings() -> None:
    """REQ-API-001: _sanitize_space_meta returns early when settings is not a dict."""
    result = _sanitize_space_meta({"id": "test", "name": "test", "settings": "bad"})
    assert result["id"] == "test"
    assert result["settings"] == "bad"


def test_ensure_space_exists_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: _ensure_space_exists returns 500 for non-notfound RuntimeError."""
    test_client.post("/spaces", json={"name": "space-ens-rt-ws"})
    with patch(
        "ugoite_core.get_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.get("/spaces/space-ens-rt-ws/entries")
    assert response.status_code == 500


def test_list_spaces_exception(test_client: TestClient) -> None:
    """REQ-API-001: list spaces returns 500 on unexpected exception."""
    with patch(
        "ugoite_core.list_spaces",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 500


def test_list_spaces_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: list spaces returns 500 on RuntimeError."""
    with patch(
        "ugoite_core.list_spaces",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 500


def test_list_spaces_skips_unauthorized_space(test_client: TestClient) -> None:
    """REQ-API-001: list spaces skips spaces the user cannot access."""
    test_client.post("/spaces", json={"name": "visible-ws"})
    test_client.post("/spaces", json={"name": "hidden-ws"})
    call_count = {"n": 0}

    async def _require_side_effect(*args: object, **kwargs: object) -> None:
        call_count["n"] += 1
        if call_count["n"] == 2:
            err_code = "forbidden"
            raise ugoite_core.AuthorizationError(err_code, "no access", "space_list")

    with patch(
        "ugoite_core.require_space_action",
        AsyncMock(side_effect=_require_side_effect),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 200
    assert len(response.json()) == 1


def test_create_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: create space returns 500 for generic RuntimeError."""
    with patch(
        "ugoite_core.create_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post("/spaces", json={"name": "fail-ws"})
    assert response.status_code == 500


def test_create_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: create space returns 500 for non-runtime exception."""
    with patch(
        "ugoite_core.create_space",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post("/spaces", json={"name": "fail-exc-ws"})
    assert response.status_code == 500


def test_get_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: get space returns 500 for generic RuntimeError."""
    test_client.post("/spaces", json={"name": "space-get-rt-ws"})
    with (
        patch("ugoite_core.require_space_action", _amock(return_value=None)),
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get("/spaces/space-get-rt-ws")
    assert response.status_code == 500


def test_get_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: get space returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "space-get-exc-ws"})
    with (
        patch("ugoite_core.require_space_action", _amock(return_value=None)),
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get("/spaces/space-get-exc-ws")
    assert response.status_code == 500


def test_patch_space_with_name(test_client: TestClient) -> None:
    """REQ-API-001: patch space can update name field."""
    test_client.post("/spaces", json={"name": "patchable-ws"})
    response = test_client.patch(
        "/spaces/patchable-ws",
        json={"name": "updated-name"},
    )
    assert response.status_code == 200


def test_patch_space_not_found_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 404 for not-found RuntimeError."""
    test_client.post("/spaces", json={"name": "patch-404-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.patch(
            "/spaces/patch-404-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 404


def test_patch_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 500 for generic RuntimeError."""
    test_client.post("/spaces", json={"name": "patch-rt-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.patch(
            "/spaces/patch-rt-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 500


def test_patch_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "patch-exc-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.patch(
            "/spaces/patch-exc-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 500


def test_test_connection_value_error(test_client: TestClient) -> None:
    """REQ-STO-006: test-connection returns 400 when storage config is invalid."""
    test_client.post("/spaces", json={"name": "conn-test-ws"})
    with patch(
        "ugoite_core.test_storage_connection",
        _amock(side_effect=ValueError("invalid storage config")),
    ):
        response = test_client.post(
            "/spaces/conn-test-ws/test-connection",
            json={"storage_config": {"uri": "invalid://bad"}},
        )
    assert response.status_code == 400


def test_test_connection_authorization_error(test_client: TestClient) -> None:
    """REQ-STO-006: test-connection returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "conn-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/conn-authz-ws/test-connection",
            json={"storage_config": {"uri": "s3://bucket"}},
        )
    assert response.status_code == 403


def test_validate_entry_form_not_found_non_error_runtime(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-004: _validate_entry_markdown_against_form returns 500.

    Applies to non-notfound form errors.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    # Use lowercase '## form' so extract_properties returns key 'form'
    with (
        patch(
            "ugoite_core.get_form",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
        pytest.raises(HTTPException) as exc_info,
    ):
        asyncio.run(
            _validate_entry_markdown_against_form(
                storage_config,
                "test-space",
                "# Note\n\n## form\nNote\n",
            ),
        )
    assert exc_info.value.status_code == 500


def test_validate_entry_form_validation_warnings(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-004: _validate_entry_markdown_against_form raises 422.

    Raised when form validation has warnings.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    fake_form = {"name": "Note", "fields": {"Body": {"type": "markdown"}}}
    fake_warnings = [{"message": "required field missing"}]
    # Use lowercase '## form' so extract_properties returns key 'form'
    with (
        patch("ugoite_core.get_form", _amock(return_value=fake_form)),
        patch(
            "ugoite_core.validate_properties",
            return_value=({}, fake_warnings),
        ),
        pytest.raises(HTTPException) as exc_info,
    ):
        asyncio.run(
            _validate_entry_markdown_against_form(
                storage_config,
                "test-space",
                "# Note\n\n## form\nNote\n",
            ),
        )
    assert exc_info.value.status_code == 422


def test_is_sql_error_function() -> None:
    """REQ-SRCH-003: _is_sql_error detects SQL error prefix."""
    assert _is_sql_error("UGOITE_SQL_ERROR: invalid syntax") is True
    assert _is_sql_error("  UGOITE_SQL_ERROR: x") is True
    assert _is_sql_error("some other error") is False


def test_query_endpoint_sql_filter_rejected(test_client: TestClient) -> None:
    """REQ-SRCH-003: query endpoint rejects SQL filter keys."""
    test_client.post("/spaces", json={"name": "query-sql-ws"})
    response = test_client.post(
        "/spaces/query-sql-ws/query",
        json={"filter": {"sql": "SELECT 1"}},
    )
    assert response.status_code == 400


def test_query_endpoint_sql_error_returns_400(test_client: TestClient) -> None:
    """REQ-SRCH-003: query endpoint returns 400 for SQL-type errors."""
    test_client.post("/spaces", json={"name": "query-sqlerr-ws"})
    with patch(
        "ugoite_core.query_index",
        _amock(side_effect=RuntimeError("UGOITE_SQL_ERROR: bad syntax")),
    ):
        response = test_client.post(
            "/spaces/query-sqlerr-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 400


def test_query_endpoint_generic_exception(test_client: TestClient) -> None:
    """REQ-SRCH-001: query endpoint returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "query-exc-ws"})
    with patch(
        "ugoite_core.query_index",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.post(
            "/spaces/query-exc-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 500


def test_query_endpoint_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: query endpoint returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "query-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "entry_read",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/query-authz-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 403


def test_search_endpoint_generic_exception(test_client: TestClient) -> None:
    """REQ-SRCH-001: search endpoint returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "search-exc-ws"})
    with patch(
        "ugoite_core.search_entries",
        _amock(side_effect=RuntimeError("index failure")),
    ):
        response = test_client.get("/spaces/search-exc-ws/search?q=test")
    assert response.status_code == 500


def test_search_endpoint_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: search endpoint returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "search-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "entry_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/search-authz-ws/search?q=test")
    assert response.status_code == 403
