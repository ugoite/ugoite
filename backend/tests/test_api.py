"""API tests."""

import base64
import hashlib
import hmac
import json
from pathlib import Path

from fastapi.testclient import TestClient


def test_create_workspace(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a new workspace."""
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 201  # noqa: PLR2004
    data = response.json()
    assert data["id"] == "test-ws"
    assert data["name"] == "test-ws"

    # Verify file system
    ws_path = temp_workspace_root / "workspaces" / "test-ws"
    assert ws_path.exists()
    assert (ws_path / "meta.json").exists()


def test_create_workspace_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test creating a workspace that already exists."""
    # Create first time
    test_client.post("/workspaces", json={"name": "test-ws"})

    # Create second time
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 409  # noqa: PLR2004
    assert "already exists" in response.json()["detail"]


def test_list_workspaces(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test listing workspaces."""
    # Create some workspaces
    test_client.post("/workspaces", json={"name": "ws1"})
    test_client.post("/workspaces", json={"name": "ws2"})

    response = test_client.get("/workspaces")
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2  # noqa: PLR2004


def test_get_workspace(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test getting a specific workspace."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    response = test_client.get("/workspaces/test-ws")
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert data["id"] == "test-ws"


def test_get_workspace_not_found(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test getting a non-existent workspace."""
    response = test_client.get("/workspaces/nonexistent")
    assert response.status_code == 404  # noqa: PLR2004


def test_create_note(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a note in a workspace."""
    # Create workspace first
    test_client.post("/workspaces", json={"name": "test-ws"})

    note_payload = {
        "content": "# My Note\n\nSome content",
    }

    response = test_client.post("/workspaces/test-ws/notes", json=note_payload)
    assert response.status_code == 201  # noqa: PLR2004
    data = response.json()
    assert "id" in data
    note_id = data["id"]

    # Verify file system
    note_path = temp_workspace_root / "workspaces" / "test-ws" / "notes" / note_id
    assert note_path.exists()
    assert (note_path / "content.json").exists()


def test_create_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 409  # noqa: PLR2004


def test_list_notes(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert isinstance(data, list)
    assert len(data) == 2  # noqa: PLR2004


def test_get_note(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test getting a specific note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# Test Note\n\nContent here"},
    )

    response = test_client.get("/workspaces/test-ws/notes/test-note")
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert data["id"] == "test-note"
    assert data["title"] == "Test Note"
    assert "# Test Note" in data["markdown"]


def test_get_note_not_found(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test getting a non-existent note."""
    test_client.post("/workspaces", json={"name": "test-ws"})

    response = test_client.get("/workspaces/test-ws/notes/nonexistent")
    assert response.status_code == 404  # noqa: PLR2004


def test_update_note(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert data["title"] == "Updated Title"
    assert "New content" in data["markdown"]


def test_update_note_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 409  # noqa: PLR2004
    # Should include the current revision for client merge
    detail = response.json()["detail"]
    assert "conflict" in str(detail).lower() or "current_revision" in str(detail)


def test_delete_note(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test deleting (tombstoning) a note."""
    test_client.post("/workspaces", json={"name": "test-ws"})
    test_client.post(
        "/workspaces/test-ws/notes",
        json={"id": "test-note", "content": "# To Delete"},
    )

    response = test_client.delete("/workspaces/test-ws/notes/test-note")
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert data["status"] == "deleted"

    # Deleted notes should not appear in list
    list_response = test_client.get("/workspaces/test-ws/notes")
    notes = list_response.json()
    note_ids = [n["id"] for n in notes]
    assert "test-note" not in note_ids


def test_get_note_history(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert "revisions" in data
    assert len(data["revisions"]) == 2  # noqa: PLR2004


def test_get_note_revision(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert data["revision_id"] == revision_id


def test_restore_note(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    data = response.json()
    assert "revision_id" in data
    assert data["restored_from"] == original_revision_id


def test_query_notes(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
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
    assert response.status_code == 200  # noqa: PLR2004
    assert isinstance(response.json(), list)


def test_middleware_headers(test_client: TestClient) -> None:
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
    assert response.status_code == 403  # noqa: PLR2004
    assert "Remote access is disabled" in response.json()["detail"]
    assert "X-IEApp-Signature" in response.headers
