"""Space membership lifecycle tests for REQ-SEC-007.

REQ-SEC-007: Space Membership Lifecycle and Invitation Collaboration.
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
def member_clients(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> Iterator[dict[str, TestClient]]:
    """Provide deterministic owner/member identities for membership tests."""
    _ = temp_space_root
    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_TOKENS_JSON",
        json.dumps(
            {
                "owner-token": {"user_id": "owner-user", "principal_type": "user"},
                "alice-token": {"user_id": "alice-user", "principal_type": "user"},
                "bob-token": {"user_id": "bob-user", "principal_type": "user"},
            },
        ),
    )
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    clear_auth_manager_cache()

    yield {
        "owner": TestClient(app, headers={"Authorization": "Bearer owner-token"}),
        "alice": TestClient(app, headers={"Authorization": "Bearer alice-token"}),
        "bob": TestClient(app, headers={"Authorization": "Bearer bob-token"}),
    }

    clear_auth_manager_cache()


def _create_space(owner_client: TestClient) -> str:
    response = owner_client.post(
        "/spaces",
        json={"name": f"members-{uuid.uuid4().hex}"},
    )
    assert response.status_code == 201
    return response.json()["id"]


def test_space_member_invite_and_accept_transitions_to_active(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-007: invited users become active members after token acceptance."""
    owner = member_clients["owner"]
    alice = member_clients["alice"]
    space_id = _create_space(owner)

    invite_response = owner.post(
        f"/spaces/{space_id}/members/invitations",
        json={"user_id": "alice-user", "role": "viewer"},
    )
    assert invite_response.status_code == 201
    token = invite_response.json()["invitation"]["token"]

    owner_space_response = owner.get(f"/spaces/{space_id}")
    assert owner_space_response.status_code == 200
    assert owner_space_response.json().get("settings", {}).get("invitations") == {}

    not_member_get = alice.get(f"/spaces/{space_id}")
    assert not_member_get.status_code == 403

    accept_response = alice.post(
        f"/spaces/{space_id}/members/accept",
        json={"token": token},
    )
    assert accept_response.status_code == 200
    assert accept_response.json()["member"]["state"] == "active"

    member_get = alice.get(f"/spaces/{space_id}")
    assert member_get.status_code == 200


def test_space_member_role_change_controls_admin_permissions(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-007: role changes alter effective admin capabilities."""
    owner = member_clients["owner"]
    bob = member_clients["bob"]
    space_id = _create_space(owner)

    invite_response = owner.post(
        f"/spaces/{space_id}/members/invitations",
        json={"user_id": "bob-user", "role": "viewer"},
    )
    assert invite_response.status_code == 201
    token = invite_response.json()["invitation"]["token"]

    accept_response = bob.post(
        f"/spaces/{space_id}/members/accept",
        json={"token": token},
    )
    assert accept_response.status_code == 200

    denied_patch = bob.patch(
        f"/spaces/{space_id}",
        json={"settings": {"default_form": "Task"}},
    )
    assert denied_patch.status_code == 403
    assert denied_patch.json()["detail"]["action"] == "space_admin"

    promote_response = owner.post(
        f"/spaces/{space_id}/members/bob-user/role",
        json={"role": "admin"},
    )
    assert promote_response.status_code == 200
    assert promote_response.json()["member"]["role"] == "admin"

    allowed_patch = bob.patch(
        f"/spaces/{space_id}",
        json={"settings": {"default_form": "Task"}},
    )
    assert allowed_patch.status_code == 200


def test_space_member_revoke_removes_access(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-007: revoked members lose access to the space."""
    owner = member_clients["owner"]
    alice = member_clients["alice"]
    space_id = _create_space(owner)

    invite_response = owner.post(
        f"/spaces/{space_id}/members/invitations",
        json={"user_id": "alice-user", "role": "editor"},
    )
    assert invite_response.status_code == 201
    token = invite_response.json()["invitation"]["token"]

    accept_response = alice.post(
        f"/spaces/{space_id}/members/accept",
        json={"token": token},
    )
    assert accept_response.status_code == 200

    revoke_response = owner.delete(f"/spaces/{space_id}/members/alice-user")
    assert revoke_response.status_code == 200
    assert revoke_response.json()["member"]["state"] == "revoked"

    revoked_get = alice.get(f"/spaces/{space_id}")
    assert revoked_get.status_code == 403
