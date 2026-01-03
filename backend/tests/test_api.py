"""API tests."""

import base64
import hashlib
import hmac
import io
import json
from pathlib import Path

from fastapi.testclient import TestClient


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

    note_payload = {
        "content": "# My Note\n\nSome content",
    }

    response = test_client.post("/workspaces/test-ws/notes", json=note_payload)
    assert response.status_code == 201
    data = response.json()
    assert "id" in data
    assert "revision_id" in data  # Required for optimistic concurrency
    note_id = data["id"]

    # Verify file system
    note_path = temp_workspace_root / "workspaces" / "test-ws" / "notes" / note_id
    assert note_path.exists()
    assert (note_path / "content.json").exists()


def test_create_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test creating a note with an existing ID (if ID is provided)."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    # Create note with specific ID
    note_id = "my-note"
    note_payload = {
        "id": note_id,
        "content": "# My Note",
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note1", "content": "# Note 1"},
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note2", "content": "# Note 2"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Test Note\n\nContent here"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Original Title"},
    )

    # Get the note to get the revision_id
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    revision_id = get_response.json()["revision_id"]

    # Update the note
    update_payload = {
        "markdown": "# Updated Title\n\nNew content",
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


def test_update_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test updating a note with a stale parent_revision_id returns 409."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Original"},
    )

    # Get the original revision_id
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    original_revision_id = get_response.json()["revision_id"]

    # First update succeeds
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "# Update 1",
            "parent_revision_id": original_revision_id,
        },
    )

    # Second update with stale revision_id should fail with 409
    response = test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "# Update 2",
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# To Delete"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Original"},
    )

    # Get initial revision
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    revision_id = get_response.json()["revision_id"]

    # Update to create another revision
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "# Updated",
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Original"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Original"},
    )

    # Get original revision
    get_response = test_client.get("/workspaces/test-ws/notes/test-note")
    original_revision_id = get_response.json()["revision_id"]

    # Update the note
    test_client.put(
        "/workspaces/test-ws/notes/test-note",
        json={
            "markdown": "# Updated",
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


def test_upload_attachment_and_link_to_note(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Attachments can be uploaded, returned with id, and linked to a note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    note_res = test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-1", "content": "# Attach Note"},
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
            "markdown": "# Attach Note\ncontent",
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
    note_res = test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-1", "content": "# Attach Note"},
    )

    response = test_client.post(
        "/workspaces/test-ws/attachments",
        files={"file": ("voice.m4a", io.BytesIO(b"data"), "audio/m4a")},
    )
    attachment = response.json()

    test_client.put(
        "/workspaces/test-ws/notes/note-1",
        json={
            "markdown": "# Attach Note\nupdated",
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-a", "content": "# A"},
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-b", "content": "# B"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-a", "content": "# A"},
    )
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "note-b", "content": "# B"},
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
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "alpha", "content": "# Alpha\nProject rocket"},
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
