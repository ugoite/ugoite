"""API tests."""

import base64
import hashlib
import hmac
import io
import json
from pathlib import Path

import pytest
import ugoite_core
from fastapi.testclient import TestClient

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


def test_create_sample_space(test_client: TestClient, temp_space_root: Path) -> None:
    """REQ-API-009: Create a sample-data space via API."""
    payload = {
        "space_id": "sample-ws",
        "scenario": "renewable-ops",
        "entry_count": 120,
        "seed": 42,
    }

    response = test_client.post("/spaces/sample-data", json=payload)
    assert response.status_code == 201
    data = response.json()
    assert data["space_id"] == "sample-ws"
    assert data["scenario"] == "renewable-ops"
    assert data["entry_count"] == 120
    assert 3 <= data["form_count"] <= 6

    list_response = test_client.get("/spaces")
    assert any(ws["id"] == "sample-ws" for ws in list_response.json())


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


def test_list_spaces_missing_root_creates_default(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """REQ-STO-009: /spaces succeeds with missing root path."""
    root = tmp_path / "missing-root"
    monkeypatch.setenv("UGOITE_ROOT", str(root))

    with TestClient(app) as client:
        response = client.get("/spaces")

    assert response.status_code == 200
    data = response.json()
    assert any(ws["id"] == "default" for ws in data)
    assert root.exists()


def test_list_spaces_handles_core_failure(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-009: /spaces returns empty list on core failure."""

    async def _raise(_config: dict[str, str]) -> list[str]:
        msg = "boom"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "list_spaces", _raise)

    response = test_client.get("/spaces")
    assert response.status_code == 200
    assert response.json() == []


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

    response = test_client.post(
        "/spaces/test-ws/sql-sessions",
        json={"sql": "SELECT * FROM entries WHERE title = 'Alpha'"},
    )
    assert response.status_code == 201
    session = response.json()
    assert session["status"] == "completed"
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


def test_create_and_list_links(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Links are created bi-directionally and listed at space level."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-a",
            "content": "---\nform: Entry\n---\n# A\n\n## Body\nA body",
        },
    )
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-b",
            "content": "---\nform: Entry\n---\n# B\n\n## Body\nB body",
        },
    )

    create_res = test_client.post(
        "/spaces/test-ws/links",
        json={"source": "entry-a", "target": "entry-b", "kind": "related"},
    )
    assert create_res.status_code == 201
    link = create_res.json()
    assert link["id"]

    list_res = test_client.get("/spaces/test-ws/links")
    assert list_res.status_code == 200
    links = list_res.json()
    assert any(link_item["id"] == link["id"] for link_item in links)

    # Entry meta files should contain the link
    entry_a = test_client.get("/spaces/test-ws/entries/entry-a").json()
    entry_b = test_client.get("/spaces/test-ws/entries/entry-b").json()
    assert any(item["target"] == "entry-b" for item in entry_a.get("links", []))
    assert any(item["target"] == "entry-a" for item in entry_b.get("links", []))


def test_delete_link_updates_entries(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Deleting a link should remove it from both entries."""
    test_client.post("/spaces", json={"name": "test-ws"})
    _create_form(test_client, "test-ws")
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-a",
            "content": "---\nform: Entry\n---\n# A\n\n## Body\nA body",
        },
    )
    test_client.post(
        "/spaces/test-ws/entries",
        json={
            "id": "entry-b",
            "content": "---\nform: Entry\n---\n# B\n\n## Body\nB body",
        },
    )

    create_res = test_client.post(
        "/spaces/test-ws/links",
        json={"source": "entry-a", "target": "entry-b", "kind": "related"},
    )
    link_id = create_res.json()["id"]

    delete_res = test_client.delete(f"/spaces/test-ws/links/{link_id}")
    assert delete_res.status_code == 200

    entry_a = test_client.get("/spaces/test-ws/entries/entry-a").json()
    entry_b = test_client.get("/spaces/test-ws/entries/entry-b").json()
    assert all(item["id"] != link_id for item in entry_a.get("links", []))
    assert all(item["id"] != link_id for item in entry_b.get("links", []))


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

    global_data = json.loads((temp_space_root / "global.json").read_text())
    secret = base64.b64decode(global_data["hmac_key"])
    expected_signature = hmac.new(
        secret,
        response.content,
        hashlib.sha256,
    ).hexdigest()

    assert response.headers["X-Ugoite-Key-Id"] == global_data["hmac_key_id"]
    assert response.headers["X-Ugoite-Signature"] == expected_signature


def test_middleware_blocks_remote_clients(test_client: TestClient) -> None:
    """Ensure remote clients are rejected unless explicitly allowed."""
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
