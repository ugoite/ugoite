"""API tests."""

import base64
import hashlib
import hmac
import io
import json
from pathlib import Path

import ieapp_core
import pytest
from fastapi.testclient import TestClient

from app.main import app


def _create_class(
    client: TestClient,
    workspace_id: str,
    class_name: str = "Note",
    fields: dict[str, dict[str, object]] | None = None,
) -> None:
    resolved_fields = fields or {"Body": {"type": "markdown"}}
    class_def = {
        "name": class_name,
        "version": 1,
        "template": f"# {class_name}\n\n## Body\n",
        "fields": resolved_fields,
    }
    res = client.post(f"/workspaces/{workspace_id}/classes", json=class_def)
    assert res.status_code == 201


def test_create_workspace(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a new workspace."""
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 201
    data = response.json()
    assert data["id"] == "test-ws"
    assert data["name"] == "test-ws"

    # Verify file system
    ws_path = temp_workspace_root / "workspaces" / "test-ws"
    assert ws_path.exists()
    assert (ws_path / "meta.json").exists()


def test_create_workspace_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test creating a workspace that already exists."""
    # Create first time
    test_client.post("/workspaces", json={"name": "test-ws"})

    # Create second time
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 409
    assert "already exists" in response.json()["detail"]


def test_list_workspaces(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test listing workspaces."""
    # Create some workspaces
    test_client.post("/workspaces", json={"name": "ws1"})
    test_client.post("/workspaces", json={"name": "ws2"})

    response = test_client.get("/workspaces")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2


def test_list_workspaces_missing_root_creates_default(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """REQ-STO-009: /workspaces succeeds with missing root path."""
    root = tmp_path / "missing-root"
    monkeypatch.setenv("IEAPP_ROOT", str(root))

    with TestClient(app) as client:
        response = client.get("/workspaces")

    assert response.status_code == 200
    data = response.json()
    assert any(ws["id"] == "default" for ws in data)
    assert root.exists()


def test_list_workspaces_handles_core_failure(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-009: /workspaces returns empty list on core failure."""

    async def _raise(_config: dict[str, str]) -> list[str]:
        msg = "boom"
        raise RuntimeError(msg)

    monkeypatch.setattr(ieapp_core, "list_workspaces", _raise)

    response = test_client.get("/workspaces")
    assert response.status_code == 200
    assert response.json() == []


def test_get_workspace(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting a specific workspace."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    response = test_client.get("/workspaces/test-ws")
    assert response.status_code == 200
    data = response.json()
    assert data["id"] == "test-ws"


def test_get_workspace_not_found(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting a non-existent workspace."""
    response = test_client.get("/workspaces/nonexistent")
    assert response.status_code == 404


def test_create_note(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a note in a workspace."""
    # Create workspace first
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")

    note_payload = {
        "content": "---\nclass: Note\n---\n# My Note\n\n## Body\nSome content",
    }

    response = test_client.post("/workspaces/test-ws/notes", json=note_payload)
    assert response.status_code == 201
    data = response.json()
    assert "id" in data
    assert "revision_id" in data  # Required for optimistic concurrency
    note_id = data["id"]

    # Verify retrieval
    get_response = test_client.get(f"/workspaces/test-ws/notes/{note_id}")
    assert get_response.status_code == 200


def test_create_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test creating a note with an existing ID (if ID is provided)."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")

    # Create note with specific ID
    note_id = "my-note"
    note_payload = {
        "id": note_id,
        "content": "---\nclass: Note\n---\n# My Note\n\n## Body\n",
    }

    test_client.post("/workspaces/test-ws/notes", json=note_payload)

    # Try again
    response = test_client.post("/workspaces/test-ws/notes", json=note_payload)
    assert response.status_code == 409


def test_list_notes(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test listing notes in a workspace."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note1",
            "content": """---
class: Note
---
# Note 1

## Body
One""",
        },
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note2",
            "content": """---
class: Note
---
# Note 2

## Body
Two""",
        },
    )

    response = test_client.get("/workspaces/test-ws/notes")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2

    # Verify NoteRecord structure includes properties and links
    for note in data:
        assert "id" in note
        assert "title" in note
        assert "properties" in note, (
            "properties field must be present in note list response"
        )
        assert "links" in note, "links field must be present in note list response"
        assert isinstance(note["properties"], dict)
        assert isinstance(note["links"], list)


def test_list_notes_workspace_not_found_returns_404(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """GET /workspaces/{id}/notes should return 404 when workspace root.

    lacks workspaces dir.
    """
    # temp_workspace_root fixture sets IEAPP_ROOT to an empty temporary directory
    response = test_client.get("/workspaces/Stay/notes")
    assert response.status_code == 404


def test_get_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting a specific note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# Test Note\n\n## Body\nContent here",
        },
    )

    response = test_client.get("/workspaces/test-ws/notes/test-note")
    assert response.status_code == 200
    data = response.json()
    assert data["id"] == "test-note"
    assert data["title"] == "Test Note"
    # Note: get_note returns "content" field (not "markdown")
    assert "# Test Note" in data["content"]


def test_get_note_not_found(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting a non-existent note."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    response = test_client.get("/workspaces/test-ws/notes/nonexistent")
    assert response.status_code == 404


def test_update_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test updating a note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": """---
class: Note
---
# Original Title

## Body
Original body""",
        },
    )

    # Get the note to get the revision_id
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    revision_id = get_response.json()["revision_id"]

    # Update the note
    update_payload = {
        "markdown": "---\nclass: Note\n---\n# Updated Title\n\n## Body\nNew content",
        "parent_revision_id": revision_id,
    }

    response = test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json=update_payload,
    )
    assert response.status_code == 200
    data = response.json()
    assert "id" in data
    assert "revision_id" in data
    assert data["revision_id"] != revision_id  # New revision

    # Verify the note was updated by fetching it
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    updated_note = get_response.json()
    assert updated_note["title"] == "Updated Title"
    # Note: get_note returns "content" field (not "markdown")
    assert "New content" in updated_note["content"]


def test_update_note_class_validation_error_returns_422_and_does_not_update(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Updating a classed note should fail with 422 when it violates the class."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    class_def = {
        "name": "Meeting",
        "version": 1,
        "template": "# Meeting\n\n## Date\n",
        "fields": {"Date": {"type": "date", "required": True}},
        "defaults": None,
    }
    res = test_client.post("/workspaces/test-ws/classes", json=class_def)
    assert res.status_code == 201

    # Create a note with class Meeting and required Date property
    note_content = """---
class: Meeting
---
# Meeting notes

## Date
2025-01-01
"""
    res = test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "meeting-1", "content": note_content},
    )
    assert res.status_code == 201

    get_res = test_client.get("/workspaces/test-ws/notes/meeting-1")
    assert get_res.status_code == 200
    original = get_res.json()
    original_revision_id = original["revision_id"]

    update_res = test_client.put(
        "/workspaces/test-ws/notes/meeting-1",
        json={
            "markdown": """---
class: Meeting
---
# Meeting notes

## Date
2025-01-01

## Extra
Nope
""",
            "parent_revision_id": original_revision_id,
        },
    )
    assert update_res.status_code == 422
    assert "Unknown class fields" in update_res.json()["detail"]

    # Ensure it did not update the revision
    get_res = test_client.get("/workspaces/test-ws/notes/meeting-1")
    assert get_res.status_code == 200
    after = get_res.json()
    assert after["revision_id"] == original_revision_id


def test_update_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test updating a note with a stale parent_revision_id returns 409."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get the original revision_id
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    original_revision_id = get_response.json()["revision_id"]

    # First update succeeds
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "---\nclass: Note\n---\n# Update 1\n\n## Body\nUpdate one",
            "parent_revision_id": original_revision_id,
        },
    )

    # Second update with stale revision_id should fail with 409
    response = test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "---\nclass: Note\n---\n# Update 2\n\n## Body\nUpdate two",
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


def test_delete_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test deleting (tombstoning) a note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# To Delete\n\n## Body\nDelete me",
        },
    )

    response = test_client.delete("/workspaces/test-ws/notes/test-note")
    assert response.status_code == 200
    data = response.json()
    assert data["status"] == "deleted"

    # Deleted notes should not appear in list
    list_response = test_client.get("/workspaces/test-ws/notes")
    notes = list_response.json()
    note_ids = [n["id"] for n in notes]
    assert "test-note" not in note_ids


def test_get_note_history(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting note history."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get initial revision
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    revision_id = get_response.json()["revision_id"]

    # Update to create another revision
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "---\nclass: Note\n---\n# Updated\n\n## Body\nUpdated",
            "parent_revision_id": revision_id,
        },
    )

    # Get history
    response = test_client.get("/workspaces/test-ws/notes/test-note/history")
    assert response.status_code == 200
    data = response.json()
    assert "revisions" in data
    assert len(data["revisions"]) == 2


def test_get_note_revision(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test getting a specific revision."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get the revision_id
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    revision_id = get_response.json()["revision_id"]

    # Get the specific revision
    response = test_client.get(
        f"/workspaces/test-ws/notes/test-note/history/{revision_id}",
    )
    assert response.status_code == 200
    data = response.json()
    assert data["revision_id"] == revision_id


def test_restore_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test restoring a note to a previous revision."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "test-note",
            "content": "---\nclass: Note\n---\n# Original\n\n## Body\nOriginal",
        },
    )

    # Get original revision
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    original_revision_id = get_response.json()["revision_id"]

    # Update the note
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "---\nclass: Note\n---\n# Updated\n\n## Body\nUpdated",
            "parent_revision_id": original_revision_id,
        },
    )

    # Restore to original
    response = test_client.post(
        "/workspaces/test-ws/notes/test-note/restore",
        json={"revision_id": original_revision_id},
    )
    assert response.status_code == 200
    data = response.json()
    assert "revision_id" in data
    assert data["restored_from"] == original_revision_id


def test_query_notes(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test structured query endpoint."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    # Create a note that should be indexed
    # Note: In a real scenario, the indexer runs in background.
    # For this test, we might need to mock the index or manually update it
    # if the API reads from index.json
    # However, the Milestone 2 implementation of `ieapp.query` should read
    # from index.json.
    # Since we haven't implemented the background indexer in the backend yet,
    # we might need to manually populate index.json or rely on the API to
    # trigger indexing (unlikely per spec).
    # Or, we just test that the endpoint exists and returns empty list for now.

    response = test_client.post("/workspaces/test-ws/query", json={"filter": {}})
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_query_notes_sql(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """REQ-IDX-008: IEapp SQL queries should return matching notes."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")

    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-sql-1",
            "content": "---\nclass: Note\n---\n# Alpha\n\n## Body\nalpha topic",
        },
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-sql-2",
            "content": "---\nclass: Note\n---\n# Beta\n\n## Body\nbeta topic",
        },
    )

    response = test_client.post(
        "/workspaces/test-ws/query",
        json={"filter": {"$sql": "SELECT * FROM notes WHERE title = 'Alpha'"}},
    )
    assert response.status_code == 200
    data = response.json()
    assert len(data) == 1
    assert data[0]["id"] == "note-sql-1"


def test_upload_attachment_and_link_to_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Attachments can be uploaded, returned with id, and linked to a note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    note_res = test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-1",
            "content": "---\nclass: Note\n---\n# Attach Note\n\n## Body\nAttach",
        },
    )
    assert note_res.status_code == 201

    file_bytes = b"hello attachment"
    response = test_client.post(
        "/workspaces/test-ws/attachments",
        files={"file": ("voice.m4a", io.BytesIO(file_bytes), "audio/m4a")},
    )
    assert response.status_code == 201
    attachment = response.json()
    assert attachment["id"]
    assert attachment["path"].startswith("attachments/")

    update_res = test_client.put(
        "/workspaces/test-ws/notes/note-1",
        json={
            "markdown": "---\nclass: Note\n---\n# Attach Note\n\n## Body\ncontent",
            "parent_revision_id": note_res.json()["revision_id"],
            "attachments": [attachment],
        },
    )
    assert update_res.status_code == 200

    # Ensure GET reflects the attachment reference
    get_res = test_client.get("/workspaces/test-ws/notes/note-1")
    assert get_res.status_code == 200
    content = get_res.json()
    assert any(a["id"] == attachment["id"] for a in content.get("attachments", []))


def test_delete_attachment_referenced_fails(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Deleting an attachment referenced by a note should fail."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    note_res = test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-1",
            "content": "---\nclass: Note\n---\n# Attach Note\n\n## Body\nAttach",
        },
    )

    response = test_client.post(
        "/workspaces/test-ws/attachments",
        files={"file": ("voice.m4a", io.BytesIO(b"data"), "audio/m4a")},
    )
    attachment = response.json()

    test_client.put(
        "/workspaces/test-ws/notes/note-1",
        json={
            "markdown": "---\nclass: Note\n---\n# Attach Note\n\n## Body\nupdated",
            "parent_revision_id": note_res.json()["revision_id"],
            "attachments": [attachment],
        },
    )

    delete_res = test_client.delete(
        f"/workspaces/test-ws/attachments/{attachment['id']}",
    )
    assert delete_res.status_code == 409
    assert "referenced" in delete_res.json()["detail"].lower()


def test_create_and_list_links(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Links are created bi-directionally and listed at workspace level."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-a",
            "content": "---\nclass: Note\n---\n# A\n\n## Body\nA body",
        },
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-b",
            "content": "---\nclass: Note\n---\n# B\n\n## Body\nB body",
        },
    )

    create_res = test_client.post(
        "/workspaces/test-ws/links",
        json={"source": "note-a", "target": "note-b", "kind": "related"},
    )
    assert create_res.status_code == 201
    link = create_res.json()
    assert link["id"]

    list_res = test_client.get("/workspaces/test-ws/links")
    assert list_res.status_code == 200
    links = list_res.json()
    assert any(link_item["id"] == link["id"] for link_item in links)

    # Note meta files should contain the link
    note_a = test_client.get("/workspaces/test-ws/notes/note-a").json()
    note_b = test_client.get("/workspaces/test-ws/notes/note-b").json()
    assert any(item["target"] == "note-b" for item in note_a.get("links", []))
    assert any(item["target"] == "note-a" for item in note_b.get("links", []))


def test_delete_link_updates_notes(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Deleting a link should remove it from both notes."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-a",
            "content": "---\nclass: Note\n---\n# A\n\n## Body\nA body",
        },
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "note-b",
            "content": "---\nclass: Note\n---\n# B\n\n## Body\nB body",
        },
    )

    create_res = test_client.post(
        "/workspaces/test-ws/links",
        json={"source": "note-a", "target": "note-b", "kind": "related"},
    )
    link_id = create_res.json()["id"]

    delete_res = test_client.delete(f"/workspaces/test-ws/links/{link_id}")
    assert delete_res.status_code == 200

    note_a = test_client.get("/workspaces/test-ws/notes/note-a").json()
    note_b = test_client.get("/workspaces/test-ws/notes/note-b").json()
    assert all(item["id"] != link_id for item in note_a.get("links", []))
    assert all(item["id"] != link_id for item in note_b.get("links", []))


def test_search_returns_matches(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Hybrid search returns notes containing the keyword via inverted index."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    _create_class(test_client, "test-ws")
    test_client.post(
        "/workspaces/test-ws/notes",
        json={
            "id": "alpha",
            "content": "---\nclass: Note\n---\n# Alpha\n\n## Body\nProject rocket",
        },
    )

    search_res = test_client.get("/workspaces/test-ws/search", params={"q": "rocket"})
    assert search_res.status_code == 200
    ids = [n["id"] for n in search_res.json()]
    assert "alpha" in ids


def test_update_workspace_storage_connector(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """PATCH workspace should persist storage connector details."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    patch_res = test_client.patch(
        "/workspaces/test-ws",
        json={
            "storage_config": {
                "uri": "s3://bucket/path",
                "credentials_profile": "default",
            },
            "settings": {"default_class": "Meeting"},
        },
    )
    assert patch_res.status_code == 200
    data = patch_res.json()
    assert data["storage_config"]["uri"] == "s3://bucket/path"
    assert data["settings"]["default_class"] == "Meeting"


def test_test_connection_endpoint(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """POST /test-connection returns success for local connector stub."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    res = test_client.post(
        "/workspaces/test-ws/test-connection",
        json={"storage_config": {"uri": "file:///tmp"}},
    )
    assert res.status_code == 200
    payload = res.json()
    assert payload["status"] == "ok"


def test_middleware_headers(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test that security headers are present."""
    response = test_client.get("/")
    assert "X-Content-Type-Options" in response.headers
    assert response.headers["X-Content-Type-Options"] == "nosniff"


def test_middleware_hmac_signature(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test that HMAC signature header matches the response body."""
    response = test_client.get("/")

    global_data = json.loads((temp_workspace_root / "global.json").read_text())
    secret = base64.b64decode(global_data["hmac_key"])
    expected_signature = hmac.new(
        secret,
        response.content,
        hashlib.sha256,
    ).hexdigest()

    assert response.headers["X-IEApp-Key-Id"] == global_data["hmac_key_id"]
    assert response.headers["X-IEApp-Signature"] == expected_signature


def test_middleware_blocks_remote_clients(test_client: TestClient) -> None:
    """Ensure remote clients are rejected unless explicitly allowed."""
    response = test_client.get("/", headers={"x-forwarded-for": "203.0.113.10"})
    assert response.status_code == 403
    assert "Remote access is disabled" in response.json()["detail"]
    assert "X-IEApp-Signature" in response.headers


def test_get_class_types(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test getting available class column types (REQ-CLS-001)."""
    # Create workspace to ensure path is valid
    test_client.post("/workspaces", json={"name": "test-ws-types"})

    response = test_client.get("/workspaces/test-ws-types/classes/types")
    assert response.status_code == 200
    data = response.json()
    assert isinstance(data, list)
    assert "string" in data
    assert "number" in data


def test_update_class_with_migration(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test updating class with migration strategies (REQ-CLS-002)."""
    # 1. Create Workspace
    test_client.post("/workspaces", json={"name": "test-ws-mig"})

    # 2. Create Initial Class
    note_class = {
        "name": "project",
        "template": "# Project",
        "fields": {
            "status": {"type": "string"},
        },
    }
    test_client.post("/workspaces/test-ws-mig/classes", json=note_class)

    # 3. Create Note
    note_payload = {
        "content": "---\nclass: project\n---\n# Project A\n\n## status\nActive\n",
    }
    # Using endpoints: POST /workspaces/{id}/notes
    # It autogenerates ID.
    res = test_client.post("/workspaces/test-ws-mig/notes", json=note_payload)
    assert res.status_code == 201
    note_id = res.json()["id"]

    # 4. Update Class with new field and migration
    updated_class = {
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
    res = test_client.post("/workspaces/test-ws-mig/classes", json=updated_class)
    assert res.status_code == 201

    # 5. Verify Note
    res = test_client.get(f"/workspaces/test-ws-mig/notes/{note_id}")
    assert res.status_code == 200
    note_data = res.json()
    content = note_data["content"]
    assert "## priority" in content
    assert "High" in content
