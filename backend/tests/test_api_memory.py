"""Tests for API using memory filesystem."""

import io
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


def test_update_note_and_search_memory(memory_client: TestClient) -> None:
    """End-to-end note update and search on memory filesystem."""
    ws_id = "mem-search"
    memory_client.post("/workspaces", json={"name": ws_id})

    create_res = memory_client.post(
        f"/workspaces/{ws_id}/notes",
        json={"id": "m1", "content": "# Title\n\nrocket launch"},
    )
    assert create_res.status_code == 201
    revision_id = create_res.json()["revision_id"]

    update_res = memory_client.put(
        f"/workspaces/{ws_id}/notes/m1",
        json={
            "markdown": "# Updated Title\n\nrocket launch scheduled",
            "parent_revision_id": revision_id,
        },
    )
    assert update_res.status_code == 200
    new_revision = update_res.json()["revision_id"]
    assert new_revision != revision_id

    search_res = memory_client.get(
        f"/workspaces/{ws_id}/search",
        params={"q": "rocket"},
    )
    assert search_res.status_code == 200
    ids = [item.get("id") for item in search_res.json()]
    assert "m1" in ids


def test_attachments_and_links_memory(memory_client: TestClient) -> None:
    """Ensure attachments and links work over memory-backed fsspec."""
    ws_id = "mem-graph"
    memory_client.post("/workspaces", json={"name": ws_id})

    note_a = memory_client.post(
        f"/workspaces/{ws_id}/notes",
        json={"id": "a", "content": "# A"},
    ).json()
    memory_client.post(
        f"/workspaces/{ws_id}/notes",
        json={"id": "b", "content": "# B"},
    )

    upload_res = memory_client.post(
        f"/workspaces/{ws_id}/attachments",
        files={"file": ("voice.m4a", io.BytesIO(b"data"), "audio/m4a")},
    )
    assert upload_res.status_code == 201
    attachment = upload_res.json()

    update_res = memory_client.put(
        f"/workspaces/{ws_id}/notes/a",
        json={
            "markdown": "# A\nwith attachment",
            "parent_revision_id": note_a["revision_id"],
            "attachments": [attachment],
        },
    )
    assert update_res.status_code == 200

    link_res = memory_client.post(
        f"/workspaces/{ws_id}/links",
        json={"source": "a", "target": "b", "kind": "related"},
    )
    assert link_res.status_code == 201
    link_id = link_res.json()["id"]

    get_a = memory_client.get(f"/workspaces/{ws_id}/notes/a")
    assert get_a.status_code == 200
    note_payload = get_a.json()
    assert any(
        att["id"] == attachment["id"] for att in note_payload.get("attachments", [])
    )
    assert any(link["id"] == link_id for link in note_payload.get("links", []))
