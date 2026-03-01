"""Tests for form management endpoints.

REQ-FORM-003: Form CRUD via API.
"""

import uuid

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app, headers={"Authorization": "Bearer test-suite-token"})


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


def test_form_req_form_007_row_reference_requires_target(space_id: str) -> None:
    """REQ-FORM-007: row_reference fields require a target_form."""
    base_form = {
        "name": "Project",
        "version": 1,
        "template": "# Project\n\n## Name\n",
        "fields": {
            "Name": {"type": "string", "required": True},
        },
    }
    response = client.post(f"/spaces/{space_id}/forms", json=base_form)
    assert response.status_code == 201

    invalid_form = {
        "name": "Task",
        "version": 1,
        "template": "# Task\n\n## Project\n",
        "fields": {
            "Project": {
                "type": "row_reference",
                "required": False,
            },
        },
    }
    response = client.post(f"/spaces/{space_id}/forms", json=invalid_form)
    assert response.status_code == 422
    detail = response.json().get("detail", "")
    assert "target_form" in detail

    valid_form = {
        "name": "Task",
        "version": 1,
        "template": "# Task\n\n## Project\n",
        "fields": {
            "Project": {
                "type": "row_reference",
                "required": False,
                "target_form": "Project",
            },
        },
    }
    response = client.post(f"/spaces/{space_id}/forms", json=valid_form)
    assert response.status_code == 201


import asyncio
from pathlib import Path
from typing import Any
from unittest.mock import AsyncMock, patch

import ugoite_core
from fastapi import HTTPException

from app.api.endpoints.forms import _persist_form_acl_settings
from app.api.endpoints.space import _format_form_validation_errors
from app.core.storage import storage_config_from_root


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_format_form_validation_errors_branches() -> None:
    """REQ-API-004: _format_form_validation_errors handles all warning shapes."""
    errors = [
        {"message": "Value is required"},
        {"field": "body_field"},
        {},
    ]
    result = _format_form_validation_errors(errors)
    assert "Value is required" in result
    assert "Invalid field: body_field" in result
    assert "Form validation error" in result


def test_persist_form_acl_settings_get_space_runtime_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-006: _persist_form_acl_settings recovers from RuntimeError.

    Handles RuntimeError from get_space gracefully.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    with (
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=RuntimeError("space not found")),
        ),
        patch(
            "ugoite_core.patch_space",
            _amock(return_value={}),
        ),
    ):
        asyncio.run(
            _persist_form_acl_settings(
                storage_config,
                "test-space",
                "Note",
                [{"type": "user", "user_id": "u1"}],
                None,
            ),
        )


def test_persist_form_acl_with_existing_acls(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-006: _persist_form_acl_settings reads existing form ACLs."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    space_meta = {
        "settings": {
            "form_acls": {
                "OtherForm": {"read": [], "write": []},
            },
        },
    }
    with (
        patch("ugoite_core.get_space", _amock(return_value=space_meta)),
        patch("ugoite_core.patch_space", _amock(return_value={})),
    ):
        asyncio.run(
            _persist_form_acl_settings(
                storage_config,
                "test-space",
                "Note",
                [{"type": "user", "user_id": "u1"}],
                None,
            ),
        )


def test_list_forms_skips_nameless_form(test_client: TestClient) -> None:
    """REQ-API-004: list forms skips forms without a valid name."""
    test_client.post("/spaces", json={"name": "forms-noname-ws"})
    fake_forms = [{"name": None}, {"name": "ValidForm", "fields": {}}]
    with (
        patch(
            "ugoite_core.list_forms",
            _amock(return_value=fake_forms),
        ),
        patch(
            "ugoite_core.require_form_read",
            _amock(return_value=None),
        ),
    ):
        response = test_client.get("/spaces/forms-noname-ws/forms")
    assert response.status_code == 200
    result = response.json()
    assert all(f.get("name") for f in result)


def test_list_forms_skips_unauthorized_form(test_client: TestClient) -> None:
    """REQ-SEC-006: list forms skips forms for which access is denied."""
    test_client.post("/spaces", json={"name": "forms-authz-ws"})
    fake_forms = [{"name": "SecretForm"}, {"name": "PublicForm"}]
    call_count = {"n": 0}

    async def _require_form_read_side_effect(*args: object, **kwargs: object) -> None:
        call_count["n"] += 1
        if call_count["n"] == 1:
            err_code = "forbidden"
            raise ugoite_core.AuthorizationError(err_code, "no access", "form_read")

    with (
        patch(
            "ugoite_core.list_forms",
            _amock(return_value=fake_forms),
        ),
        patch(
            "ugoite_core.require_form_read",
            AsyncMock(side_effect=_require_form_read_side_effect),
        ),
    ):
        response = test_client.get("/spaces/forms-authz-ws/forms")
    assert response.status_code == 200
    # Only the non-denied form should appear
    assert len(response.json()) == 1


def test_list_forms_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: list forms returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "forms-err-ws"})
    with patch(
        "ugoite_core.list_forms",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/forms-err-ws/forms")
    assert response.status_code == 500


def test_list_forms_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list forms returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "forms-outer-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/forms-outer-authz-ws/forms")
    assert response.status_code == 403


def test_list_form_types_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: list form types returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "formtypes-err-ws"})
    with patch(
        "ugoite_core.list_column_types",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/formtypes-err-ws/forms/types")
    assert response.status_code == 500


def test_list_form_types_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list form types returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "formtypes-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/formtypes-authz-ws/forms/types")
    assert response.status_code == 403


def test_list_form_types_reraises_http_exception(test_client: TestClient) -> None:
    """REQ-API-004: list form types re-raises HTTPException from column types call."""
    test_client.post("/spaces", json={"name": "formtypes-httpexc-ws"})
    with patch(
        "ugoite_core.list_column_types",
        _amock(side_effect=HTTPException(status_code=503, detail="unavailable")),
    ):
        response = test_client.get("/spaces/formtypes-httpexc-ws/forms/types")
    assert response.status_code == 503


def test_get_form_not_found(test_client: TestClient) -> None:
    """REQ-API-004: get form returns 404 when form does not exist."""
    test_client.post("/spaces", json={"name": "form-get-404-ws"})
    response = test_client.get("/spaces/form-get-404-ws/forms/MissingForm")
    assert response.status_code == 404


def test_get_form_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-004: get form returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "form-get-rt-ws"})
    with (
        patch("ugoite_core.require_form_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_form",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get("/spaces/form-get-rt-ws/forms/SomeForm")
    assert response.status_code == 500


def test_get_form_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get form returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "form-authz-ws"})
    with patch(
        "ugoite_core.require_form_read",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "form_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/form-authz-ws/forms/SecretForm")
    assert response.status_code == 403


def test_create_form_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: create form returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "form-create-exc-ws"})
    with patch(
        "ugoite_core.upsert_form",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/form-create-exc-ws/forms",
            json={
                "name": "TestForm",
                "version": 1,
                "template": "# TestForm\n",
                "fields": {"Body": {"type": "markdown"}},
            },
        )
    assert response.status_code == 500


def test_create_form_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-004: create form returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "form-create-rt-ws"})
    with patch(
        "ugoite_core.upsert_form",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/form-create-rt-ws/forms",
            json={
                "name": "TestForm",
                "version": 1,
                "template": "# TestForm\n",
                "fields": {"Body": {"type": "markdown"}},
            },
        )
    assert response.status_code == 500
