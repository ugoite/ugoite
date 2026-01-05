"""Tests for API using memory filesystem."""

from collections.abc import Generator

import pytest
from fastapi.testclient import TestClient

from app.main import app

# We need to patch get_root_path or set env var before app startup?
# TestClient starts the app.
# But app startup logic runs when TestClient is created (or first request).


@pytest.fixture
def memory_client(
    monkeypatch: pytest.MonkeyPatch,
) -> Generator[TestClient]:
    """Create a TestClient with memory filesystem."""
    # Use a unique memory root for this test session
    memory_root = "memory://test_backend_root"

    # Patch environment variable
    monkeypatch.setenv("IEAPP_ROOT", memory_root)

    # We also need to make sure get_root_path returns this
    # (It reads env var, so it should be fine if patched before call)

    # Create a new TestClient to trigger startup with new env
    with TestClient(app) as client:
        yield client


def test_create_workspace_memory(memory_client: TestClient) -> None:
    """Test creating a workspace in memory fs."""
    response = memory_client.post("/workspaces", json={"name": "mem-ws"})
    assert response.status_code == 201
    data = response.json()
    assert data["id"] == "mem-ws"

    # Verify it exists in list
    response = memory_client.get("/workspaces")
    assert response.status_code == 200
    workspaces = response.json()
    assert any(ws["id"] == "mem-ws" for ws in workspaces)


def test_create_note_memory(memory_client: TestClient) -> None:
    """Test creating a note in memory fs."""
    # Create workspace first
    ws_id = "note-ws"
    memory_client.post("/workspaces", json={"name": ws_id})

    # Create note
    note_payload = {"content": "# Memory Note\n\nStored in RAM."}
    response = memory_client.post(f"/workspaces/{ws_id}/notes", json=note_payload)
    assert response.status_code == 201
    note_data = response.json()
    note_id = note_data["id"]

    # Get note
    response = memory_client.get(f"/workspaces/{ws_id}/notes/{note_id}")
    assert response.status_code == 200
    assert response.json()["content"] == note_payload["content"]
