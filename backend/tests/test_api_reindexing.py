"""Test API reindexing logic."""

from pathlib import Path

from fastapi.testclient import TestClient


def test_update_note_reflects_in_query(
    test_client: TestClient,
    temp_workspace_root: Path,
) -> None:
    """Test that updating a note reflects in the query index (REQ-IDX-001)."""
    # 1. Create Workspace
    ws_id = "reindex-test-ws"
    test_client.post("/api/workspaces", json={"name": ws_id})

    # 2. Create Class
    note_class = {
        "name": "Task",
        "version": 1,
        "template": "# Task\n\n## priority\n",
        "fields": {
            "priority": {"type": "string"},
        },
    }
    test_client.post(f"/api/workspaces/{ws_id}/classes", json=note_class)

    # 3. Create Note with Class
    note_payload = {
        "title": "My Task",
        "content": "---\nclass: Task\n---\n# My Task\n\n## priority\nhigh\n",
    }
    res = test_client.post(f"/api/workspaces/{ws_id}/notes", json=note_payload)
    assert res.status_code == 201
    note_id = res.json()["id"]
    rev_id = res.json()["revision_id"]

    # 4. Query - should be high
    # NOTE: In a real environment, indexing might be async.
    # The current create_note might trigger sync indexing or not.
    # Let's see if query returns expected result.
    res = test_client.post(
        f"/api/workspaces/{ws_id}/query",
        json={"filter": {"class": "Task"}},
    )
    assert res.status_code == 200
    notes = res.json()
    assert len(notes) == 1
    assert notes[0]["properties"]["priority"] == "high"

    # 5. Update Note - change priority to low
    update_payload = {
        "markdown": "---\nclass: Task\n---\n# My Task\n\n## priority\nlow\n",
        "parent_revision_id": rev_id,
    }
    res = test_client.put(
        f"/api/workspaces/{ws_id}/notes/{note_id}",
        json=update_payload,
    )
    assert res.status_code == 200

    # 6. Query again - should be low
    res = test_client.post(
        f"/api/workspaces/{ws_id}/query",
        json={"filter": {"class": "Task"}},
    )
    assert res.status_code == 200
    notes = res.json()
    assert len(notes) == 1
    # This is where we expect it to fail if reindexing is not triggered
    assert notes[0]["properties"]["priority"] == "low"
