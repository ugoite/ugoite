"""Tests for form management endpoints.

REQ-FORM-003: Form CRUD via API.
"""

import uuid

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)


@pytest.fixture
def space_id(tmp_path: object) -> str:
    """Create a space for testing."""
    ws_name = f"form-test-ws-{uuid.uuid4().hex}"
    response = client.post("/spaces", json={"name": ws_name})
    assert response.status_code == 201
    return response.json()["id"]


def test_create_and_get_form(space_id: str) -> None:
    """Test creating and retrieving a form."""
    form_name = "Meeting"
    form_def = {
        "name": form_name,
        "version": 1,
        "template": "# Meeting\n\n## Date\n## Attendees\n",
        "fields": {
            "Date": {"type": "date", "required": True},
            "Attendees": {"type": "list", "required": False},
        },
        "defaults": None,
    }
    expected_template = "# Meeting\n\n## Attendees\n\n## Date\n\n"

    # Create Form
    response = client.post(f"/spaces/{space_id}/forms", json=form_def)
    assert response.status_code == 201
    assert response.json()["name"] == form_name

    # Get Form
    response = client.get(f"/spaces/{space_id}/forms/{form_name}")
    assert response.status_code == 200
    payload = response.json()
    assert payload["name"] == form_name
    assert payload["fields"] == form_def["fields"]
    assert payload["template"] == expected_template

    # List Forms
    response = client.get(f"/spaces/{space_id}/forms")
    assert response.status_code == 200
    forms = response.json()
    assert len(forms) == 1
    assert forms[0]["name"] == form_name


def test_form_validation_in_entry(space_id: str) -> None:
    """Test that entries created with a form have their properties extracted."""
    # 1. Define Form
    form_name = "Task"
    form_def = {
        "name": form_name,
        "template": "# Task",
        "fields": {
            "Priority": {"type": "string", "required": True},
        },
    }
    client.post(f"/spaces/{space_id}/forms", json=form_def)

    # 2. Create Entry with that Form
    entry_content = """---
form: Task
---
## Priority
High
"""
    response = client.post(
        f"/spaces/{space_id}/entries",
        json={"content": entry_content},
    )
    assert response.status_code == 201

    # 3. Query to check if properties are extracted
    # We need to trigger indexing first. In tests, we might need to wait or force index.
    # The search endpoint forces index run_once.
    client.get(f"/spaces/{space_id}/search?q=Task")

    # Now query
    response = client.post(
        f"/spaces/{space_id}/query",
        json={"filter": {"form": "Task"}},
    )
    assert response.status_code == 200
    results = response.json()
    assert len(results) == 1
    assert results[0]["properties"]["Priority"] == "High"


def test_update_entry_with_missing_form_rejected(space_id: str) -> None:
    """Updating a entry that declares a form whose form file is missing.

    This should fail.
    """
    # Create a valid form and entry first
    form_def = {
        "name": "Task",
        "template": "# Task",
        "fields": {
            "Field": {"type": "string", "required": False},
        },
    }
    client.post(f"/spaces/{space_id}/forms", json=form_def)

    entry_content = """---
form: Task
---
## Field
Value
"""
    create_resp = client.post(
        f"/spaces/{space_id}/entries",
        json={"content": entry_content},
    )
    assert create_resp.status_code == 201
    entry_id = create_resp.json()["id"]
    revision_id = create_resp.json()["revision_id"]

    # Attempt to update with a missing form
    updated_md = """---
form: MissingForm
---
## Field
Value
"""
    upd_resp = client.put(
        f"/spaces/{space_id}/entries/{entry_id}",
        json={
            "markdown": updated_md,
            "parent_revision_id": revision_id,
        },
    )
    assert upd_resp.status_code == 422


def test_create_reserved_metadata_form_rejected(space_id: str) -> None:
    """REQ-FORM-006: Reserved metadata forms are rejected via API."""
    form_def = {
        "name": "SQL",
        "version": 1,
        "template": "# SQL\n\n## sql\n\n## variables\n",
        "fields": {
            "sql": {"type": "string", "required": True},
            "variables": {"type": "object_list", "required": False},
        },
    }

    response = client.post(f"/spaces/{space_id}/forms", json=form_def)
    assert response.status_code == 422
    detail = response.json().get("detail", "")
    assert "reserved" in detail.lower()
