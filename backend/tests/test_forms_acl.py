"""Authorization tests for REQ-SEC-006.

REQ-SEC-006: Space-Scoped Authorization and Form ACL.
"""

from __future__ import annotations

import json
import uuid
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache
from app.main import app


@pytest.fixture
def auth_clients(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> Iterator[dict[str, TestClient]]:
    """Provide deterministic multi-user clients for authz tests."""
    _ = temp_space_root
    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_TOKENS_JSON",
        json.dumps(
            {
                "owner-token": {"user_id": "owner-user", "principal_type": "user"},
                "viewer-token": {
                    "user_id": "viewer-user",
                    "principal_type": "user",
                },
            },
        ),
    )
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    clear_auth_manager_cache()

    yield {
        "owner": TestClient(app, headers={"Authorization": "Bearer owner-token"}),
        "viewer": TestClient(app, headers={"Authorization": "Bearer viewer-token"}),
    }

    clear_auth_manager_cache()


def _create_space(owner_client: TestClient, name: str) -> str:
    response = owner_client.post("/spaces", json={"name": f"{name}-{uuid.uuid4().hex}"})
    assert response.status_code == 201
    return response.json()["id"]


def _create_form_with_acl(owner_client: TestClient, space_id: str) -> None:
    form_payload = {
        "name": "Task",
        "version": 1,
        "template": "# Task\n\n## Summary\n",
        "fields": {"Summary": {"type": "string", "required": True}},
        "read_principals": [{"kind": "user", "id": "owner-user"}],
        "write_principals": [{"kind": "user", "id": "owner-user"}],
    }
    response = owner_client.post(f"/spaces/{space_id}/forms", json=form_payload)
    assert response.status_code == 201


def _create_task_entry(owner_client: TestClient, space_id: str) -> str:
    content = """---
form: Task
---
## Summary
Restricted task
"""
    response = owner_client.post(
        f"/spaces/{space_id}/entries",
        json={"content": content},
    )
    assert response.status_code == 201
    return response.json()["id"]


def test_form_acl_denies_unauthorized_read_access(
    auth_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: unauthorized users cannot read restricted Forms."""
    owner = auth_clients["owner"]
    viewer = auth_clients["viewer"]
    space_id = _create_space(owner, "acl-read-ws")
    _create_form_with_acl(owner, space_id)
    entry_id = _create_task_entry(owner, space_id)

    get_response = viewer.get(f"/spaces/{space_id}/entries/{entry_id}")
    assert get_response.status_code == 403
    assert get_response.json()["detail"]["code"] == "forbidden"

    list_response = viewer.get(f"/spaces/{space_id}/entries")
    assert list_response.status_code == 403
    assert list_response.json()["detail"]["action"] == "space_read"


def test_form_acl_denies_unauthorized_write_access(
    auth_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: unauthorized users cannot write restricted Forms."""
    owner = auth_clients["owner"]
    viewer = auth_clients["viewer"]
    space_id = _create_space(owner, "acl-write-ws")

    patch_response = owner.patch(
        f"/spaces/{space_id}",
        json={
            "settings": {
                "member_roles": {
                    "owner-user": "owner",
                    "viewer-user": "viewer",
                },
            },
        },
    )
    assert patch_response.status_code == 200

    form_payload = {
        "name": "EditorOnly",
        "version": 1,
        "template": "# EditorOnly\n\n## Name\n",
        "fields": {"Name": {"type": "string", "required": True}},
    }
    owner_create = owner.post(f"/spaces/{space_id}/forms", json=form_payload)
    assert owner_create.status_code == 201

    viewer_create = viewer.post(f"/spaces/{space_id}/forms", json=form_payload)
    assert viewer_create.status_code == 403
    assert viewer_create.json()["detail"]["action"] == "form_write"


def test_form_acl_allows_group_principal_access(
    auth_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: UserGroup principal access can be granted."""
    owner = auth_clients["owner"]
    viewer = auth_clients["viewer"]
    space_id = _create_space(owner, "acl-group-ws")

    patch_response = owner.patch(
        f"/spaces/{space_id}",
        json={
            "settings": {
                "user_groups": {"viewer-user": ["eng"]},
                "member_roles": {
                    "owner-user": "owner",
                    "viewer-user": "viewer",
                },
            },
        },
    )
    assert patch_response.status_code == 200

    form_payload = {
        "name": "GroupReadable",
        "version": 1,
        "template": "# GroupReadable\n\n## Summary\n",
        "fields": {"Summary": {"type": "string", "required": True}},
        "read_principals": [{"kind": "user_group", "id": "eng"}],
        "write_principals": [{"kind": "user", "id": "owner-user"}],
    }
    create_form = owner.post(f"/spaces/{space_id}/forms", json=form_payload)
    assert create_form.status_code == 201

    create_entry = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "content": "---\nform: GroupReadable\n---\n## Summary\nShared\n",
        },
    )
    assert create_entry.status_code == 201

    entry_id = create_entry.json()["id"]
    viewer_get = viewer.get(f"/spaces/{space_id}/entries/{entry_id}")
    assert viewer_get.status_code == 200


def test_materialized_view_inherits_form_acl_policies(
    auth_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: prevent privilege escalation on space policy mutation."""
    owner = auth_clients["owner"]
    viewer = auth_clients["viewer"]
    space_id = _create_space(owner, "acl-escalation-ws")

    response = viewer.patch(
        f"/spaces/{space_id}",
        json={"settings": {"member_roles": {"viewer-user": "owner"}}},
    )
    assert response.status_code == 403
    assert response.json()["detail"]["action"] == "space_read"
