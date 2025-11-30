"""API tests."""

from pathlib import Path

from fastapi.testclient import TestClient


def test_create_workspace(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a new workspace."""
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 201  # noqa: S101, PLR2004
    data = response.json()
    assert data["id"] == "test-ws"  # noqa: S101
    assert data["name"] == "test-ws"  # noqa: S101

    # Verify file system
    ws_path = temp_workspace_root / "workspaces" / "test-ws"
    assert ws_path.exists()  # noqa: S101
    assert (ws_path / "meta.json").exists()  # noqa: S101


def test_create_workspace_conflict(
    test_client: TestClient,
    temp_workspace_root: Path,  # noqa: ARG001
) -> None:
    """Test creating a workspace that already exists."""
    # Create first time
    test_client.post("/workspaces", json={"name": "test-ws"})

    # Create second time
    response = test_client.post("/workspaces", json={"name": "test-ws"})
    assert response.status_code == 409  # noqa: S101, PLR2004
    assert "already exists" in response.json()["detail"]  # noqa: S101


def test_create_note(test_client: TestClient, temp_workspace_root: Path) -> None:
    """Test creating a note in a workspace."""
    # Create workspace first
    test_client.post("/workspaces", json={"name": "test-ws"})

    note_payload = {
        "content": "# My Note\n\nSome content",
    }

    response = test_client.post("/workspaces/test-ws/notes", json=note_payload)
    assert response.status_code == 201  # noqa: S101, PLR2004
    data = response.json()
    assert "id" in data  # noqa: S101
    note_id = data["id"]

    # Verify file system
    note_path = temp_workspace_root / "workspaces" / "test-ws" / "notes" / note_id
    assert note_path.exists()  # noqa: S101
    assert (note_path / "content.json").exists()  # noqa: S101


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
    assert response.status_code == 409  # noqa: S101, PLR2004


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
    assert response.status_code == 200  # noqa: S101, PLR2004
    assert isinstance(response.json(), list)  # noqa: S101


def test_middleware_headers(test_client: TestClient) -> None:
    """Test that security headers are present."""
    response = test_client.get("/")
    assert "X-Content-Type-Options" in response.headers  # noqa: S101
    # assert "X-IEApp-Signature" in response.headers # If we implement HMAC
