"""Tests for API using memory filesystem.

REQ-STO-007: Backend IO separation & multi-fsspec coverage.
"""

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
    monkeypatch.setenv("UGOITE_ROOT", memory_root)

    # We also need to make sure get_root_path returns this
    # (It reads env var, so it should be fine if patched before call)

    # Create a new TestClient to trigger startup with new env
    with TestClient(app) as client:
        yield client


def test_create_space_memory(memory_client: TestClient) -> None:
    """Test creating a space in memory fs."""
    response = memory_client.post("/spaces", json={"name": "mem-ws"})
    assert response.status_code == 201
    data = response.json()
    assert data["id"] == "mem-ws"

    # Verify it exists in list
    response = memory_client.get("/spaces")
    assert response.status_code == 200
    spaces = response.json()
    assert any(ws["id"] == "mem-ws" for ws in spaces)


def test_create_entry_memory(memory_client: TestClient) -> None:
    """Test creating a entry in memory fs."""
    # Create space first
    ws_id = "entry-ws"
    memory_client.post("/spaces", json={"name": ws_id})
    memory_client.post(
        f"/spaces/{ws_id}/forms",
        json={
            "name": "Entry",
            "version": 1,
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )

    # Create entry
    entry_payload = {
        "content": "---\nform: Entry\n---\n# Memory Entry\n\n## Body\nStored in RAM.",
    }
    response = memory_client.post(f"/spaces/{ws_id}/entries", json=entry_payload)
    assert response.status_code == 201
    entry_data = response.json()
    entry_id = entry_data["id"]

    # Get entry
    response = memory_client.get(f"/spaces/{ws_id}/entries/{entry_id}")
    assert response.status_code == 200
    assert response.json()["content"] == entry_payload["content"]


def test_update_entry_and_search_memory(memory_client: TestClient) -> None:
    """End-to-end entry update and search on memory filesystem."""
    ws_id = "mem-search"
    memory_client.post("/spaces", json={"name": ws_id})
    memory_client.post(
        f"/spaces/{ws_id}/forms",
        json={
            "name": "Entry",
            "version": 1,
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )

    create_res = memory_client.post(
        f"/spaces/{ws_id}/entries",
        json={
            "id": "m1",
            "content": "---\nform: Entry\n---\n# Title\n\n## Body\nrocket launch",
        },
    )
    assert create_res.status_code == 201
    revision_id = create_res.json()["revision_id"]

    update_res = memory_client.put(
        f"/spaces/{ws_id}/entries/m1",
        json={
            "markdown": """---
form: Entry
---
# Updated Title

## Body
rocket launch scheduled""",
            "parent_revision_id": revision_id,
        },
    )
    assert update_res.status_code == 200
    new_revision = update_res.json()["revision_id"]
    assert new_revision != revision_id

    search_res = memory_client.get(
        f"/spaces/{ws_id}/search",
        params={"q": "rocket"},
    )
    assert search_res.status_code == 200
    ids = [item.get("id") for item in search_res.json()]
    assert "m1" in ids


def test_assets_and_links_memory(memory_client: TestClient) -> None:
    """Ensure assets and links work over memory-backed fsspec."""
    ws_id = "mem-graph"
    memory_client.post("/spaces", json={"name": ws_id})
    memory_client.post(
        f"/spaces/{ws_id}/forms",
        json={
            "name": "Entry",
            "version": 1,
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )

    entry_a = memory_client.post(
        f"/spaces/{ws_id}/entries",
        json={"id": "a", "content": "---\nform: Entry\n---\n# A\n\n## Body\nA body"},
    ).json()
    memory_client.post(
        f"/spaces/{ws_id}/entries",
        json={"id": "b", "content": "---\nform: Entry\n---\n# B\n\n## Body\nB body"},
    )

    upload_res = memory_client.post(
        f"/spaces/{ws_id}/assets",
        files={"file": ("voice.m4a", io.BytesIO(b"data"), "audio/m4a")},
    )
    assert upload_res.status_code == 201
    asset = upload_res.json()

    update_res = memory_client.put(
        f"/spaces/{ws_id}/entries/a",
        json={
            "markdown": "---\nform: Entry\n---\n# A\n\n## Body\nwith asset",
            "parent_revision_id": entry_a["revision_id"],
            "assets": [asset],
        },
    )
    assert update_res.status_code == 200

    link_res = memory_client.post(
        f"/spaces/{ws_id}/links",
        json={"source": "a", "target": "b", "kind": "related"},
    )
    assert link_res.status_code == 201
    link_id = link_res.json()["id"]

    get_a = memory_client.get(f"/spaces/{ws_id}/entries/a")
    assert get_a.status_code == 200
    entry_payload = get_a.json()
    assert any(att["id"] == asset["id"] for att in entry_payload.get("assets", []))
    assert any(link["id"] == link_id for link in entry_payload.get("links", []))
