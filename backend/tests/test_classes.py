"""Tests for class management endpoints."""

import uuid

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)


@pytest.fixture
def workspace_id(tmp_path: object) -> str:
    """Create a workspace for testing."""
    ws_name = f"class-test-ws-{uuid.uuid4().hex}"
    response = client.post("/workspaces", json={"name": ws_name})
    assert response.status_code == 201
    return response.json()["id"]


def test_create_and_get_class(workspace_id: str) -> None:
    """Test creating and retrieving a class."""
    class_name = "Meeting"
    class_def = {
        "name": class_name,
        "version": 1,
        "template": "# Meeting\n\n## Date\n## Attendees\n",
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        },
        "defaults": None,
    }

    # Create Class
    response = client.post(f"/workspaces/{workspace_id}/classes", json=class_def)
    assert response.status_code == 201
    assert response.json()["name"] == class_name

    # Get Class
    response = client.get(f"/workspaces/{workspace_id}/classes/{class_name}")
    assert response.status_code == 200
    assert response.json() == class_def

    # List Classes
    response = client.get(f"/workspaces/{workspace_id}/classes")
    assert response.status_code == 200
    classes = response.json()
    assert len(classes) == 1
    assert classes[0]["name"] == class_name


def test_class_validation_in_note(workspace_id: str) -> None:
    """Test that notes created with a class have their properties extracted."""
    # 1. Define Class
    class_name = "Task"
    class_def = {
        "name": class_name,
        "fields": {
            "Priority": {"type": "string", "required": True},
        },
    }
    client.post(f"/workspaces/{workspace_id}/classes", json=class_def)

    # 2. Create Note with that Class
    note_content = """---
class: Task
---
## Priority
High
"""
    response = client.post(
        f"/workspaces/{workspace_id}/notes",
        json={"content": note_content},
    )
    assert response.status_code == 201

    # 3. Query to check if properties are extracted
    # We need to trigger indexing first. In tests, we might need to wait or force index.
    # The search endpoint forces index run_once.
    client.get(f"/workspaces/{workspace_id}/search?q=Task")

    # Now query
    response = client.post(
        f"/workspaces/{workspace_id}/query",
        json={"filter": {"class": "Task"}},
    )
    assert response.status_code == 200
    results = response.json()
    assert len(results) == 1
    assert results[0]["properties"]["Priority"] == "High"


def test_update_note_with_missing_class_rejected(workspace_id: str) -> None:
    """Updating a note that declares a class whose class file is missing.

    This should fail.
    """
    # Create a note that references a non-existent class
    note_content = """---
class: MissingClass
---
## Field
Value
"""
    create_resp = client.post(
        f"/workspaces/{workspace_id}/notes",
        json={"content": note_content},
    )
    assert create_resp.status_code == 201
    note_id = create_resp.json()["id"]
    revision_id = create_resp.json()["revision_id"]

    # Attempt to update the note (this triggers class validation)
    updated_md = note_content + "\n## Another\nX"
    upd_resp = client.put(
        f"/workspaces/{workspace_id}/notes/{note_id}",
        json={
            "markdown": updated_md,
            "parent_revision_id": revision_id,
        },
    )
    assert upd_resp.status_code == 422
