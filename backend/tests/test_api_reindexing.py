"""Test API reindexing logic."""

from pathlib import Path

from fastapi.testclient import TestClient


def test_update_entry_reflects_in_query(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """Test that updating a entry reflects in the query index (REQ-IDX-001)."""
    # 1. Create Space
    ws_id = "reindex-test-ws"
    test_client.post("/spaces", json={"name": ws_id})

    # 2. Create Form
    entry_form = {
        "name": "Task",
        "version": 1,
        "template": "# Task\n\n## priority\n",
        "fields": {
            "priority": {"type": "string"},
        },
    }
    test_client.post(f"/spaces/{ws_id}/forms", json=entry_form)

    # 3. Create Entry with Form
    entry_payload = {
        "title": "My Task",
        "content": "---\nform: Task\n---\n# My Task\n\n## priority\nhigh\n",
    }
    res = test_client.post(f"/spaces/{ws_id}/entries", json=entry_payload)
    assert res.status_code == 201
    entry_id = res.json()["id"]
    rev_id = res.json()["revision_id"]

    # 4. Query - should be high
    # NOTE: In a real environment, indexing might be async.
    # The current create_entry might trigger sync indexing or not.
    # Let's see if query returns expected result.
    res = test_client.post(
        f"/spaces/{ws_id}/query",
        json={"filter": {"form": "Task"}},
    )
    assert res.status_code == 200
    entries = res.json()
    assert len(entries) == 1
    assert entries[0]["properties"]["priority"] == "high"

    # 5. Update Entry - change priority to low
    update_payload = {
        "markdown": "---\nform: Task\n---\n# My Task\n\n## priority\nlow\n",
        "parent_revision_id": rev_id,
    }
    res = test_client.put(f"/spaces/{ws_id}/entries/{entry_id}", json=update_payload)
    assert res.status_code == 200

    # 6. Query again - should be low
    res = test_client.post(
        f"/spaces/{ws_id}/query",
        json={"filter": {"form": "Task"}},
    )
    assert res.status_code == 200
    entries = res.json()
    assert len(entries) == 1
    # This is where we expect it to fail if reindexing is not triggered
    assert entries[0]["properties"]["priority"] == "low"
