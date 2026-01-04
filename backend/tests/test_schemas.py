"""Tests for schema management endpoints."""

import uuid

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)


@pytest.fixture
def workspace_id(tmp_path: object) -> str:
    """Create a workspace for testing."""
    ws_name = f"schema-test-ws-{uuid.uuid4().hex}"
    response = client.post("/workspaces", json={"name": ws_name})
    assert response.status_code == 201
    return response.json()["id"]


def test_create_and_get_schema(workspace_id: str) -> None:
    """Test creating and retrieving a schema."""
    schema_name = "Meeting"
    schema_def = {
        "name": schema_name,
        "version": 1,
        "template": "# Meeting\n\n## Date\n## Attendees\n",
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        },
        "defaults": None,
    }

    # Create Schema
    response = client.post(f"/workspaces/{workspace_id}/schemas", json=schema_def)
    assert response.status_code == 201
    assert response.json()["name"] == schema_name

    # Get Schema
    response = client.get(f"/workspaces/{workspace_id}/schemas/{schema_name}")
    assert response.status_code == 200
    assert response.json() == schema_def

    # List Schemas
    response = client.get(f"/workspaces/{workspace_id}/schemas")
    assert response.status_code == 200
    schemas = response.json()
    assert len(schemas) == 1
    assert schemas[0]["name"] == schema_name


def test_schema_validation_in_note(workspace_id: str) -> None:
    """Test that notes created with a class have their properties extracted."""
    # 1. Define Schema
    schema_name = "Task"
    schema_def = {
        "name": schema_name,
        "fields": {
            "Priority": {"type": "string", "required": True},
        },
    }
    client.post(f"/workspaces/{workspace_id}/schemas", json=schema_def)

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


def test_update_note_with_missing_schema_rejected(workspace_id: str) -> None:
    """Updating a note that declares a class whose schema file is missing.

    This should fail.
    """
    # Create a note that references a non-existent class
    note_content = """---
class: MissingSchema
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

    # Attempt to update the note (this triggers schema validation)
    updated_md = note_content + "\n## Another\nX"
    upd_resp = client.put(
        f"/workspaces/{workspace_id}/notes/{note_id}",
        json={
            "markdown": updated_md,
            "parent_revision_id": revision_id,
        },
    )
    assert upd_resp.status_code == 422
