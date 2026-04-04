"""Space membership lifecycle tests for REQ-SEC-007.

REQ-SEC-007: Space Membership Lifecycle and Invitation Collaboration.
"""

from __future__ import annotations

import asyncio
import json
import uuid
from collections.abc import Iterator
from pathlib import Path

import pytest
import ugoite_core
from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache
from app.core.storage import storage_config_from_root
from app.main import app


def _bootstrap_admin_space_for_user(temp_space_root: Path, user_id: str) -> None:
    asyncio.run(
        ugoite_core.ensure_admin_space(
            storage_config_from_root(temp_space_root),
            user_id,
        ),
    )


@pytest.fixture
def member_clients(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> Iterator[dict[str, TestClient]]:
    """Provide deterministic owner/member identities for membership tests."""
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
    _bootstrap_admin_space_for_user(temp_space_root, "owner-user")
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


def test_space_creator_is_bootstrapped_as_active_admin(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: space creator becomes the initial active admin member."""
    owner = member_clients["owner"]
    space_id = _create_space(owner)

    response = owner.get(f"/spaces/{space_id}")
    assert response.status_code == 200
    members = response.json()["settings"]["members"]
    assert members["owner-user"]["role"] == "admin"
    assert members["owner-user"]["state"] == "active"


def test_space_rejects_demoting_last_active_admin(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: a space must retain at least one active admin."""
    owner = member_clients["owner"]
    space_id = _create_space(owner)

    response = owner.post(
        f"/spaces/{space_id}/members/owner-user/role",
        json={"role": "viewer"},
    )

    assert response.status_code == 409
    assert "at least one active admin" in response.json()["detail"]


def test_space_rejects_revoking_last_active_admin(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-006: revocation cannot remove the last active admin."""
    owner = member_clients["owner"]
    space_id = _create_space(owner)

    response = owner.delete(f"/spaces/{space_id}/members/owner-user")

    assert response.status_code == 409
    assert "at least one active admin" in response.json()["detail"]


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


def test_space_patch_rejects_membership_managed_settings(
    member_clients: dict[str, TestClient],
) -> None:
    """REQ-SEC-007: generic space patch rejects membership-managed settings keys."""
    owner = member_clients["owner"]
    space_id = _create_space(owner)

    with patch("ugoite_core.patch_space", _amock()) as patch_space:
        response = owner.patch(
            f"/spaces/{space_id}",
            json={
                "settings": {
                    "members": {
                        "ghost-user": {
                            "user_id": "ghost-user",
                            "role": "admin",
                            "state": "active",
                        }
                    }
                }
            },
        )

    assert response.status_code == 422
    assert "membership-managed keys" in response.json()["detail"]
    assert "members" in response.json()["detail"]
    patch_space.assert_not_awaited()


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


from typing import Any
from unittest.mock import AsyncMock, patch


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_list_members_success(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns the owner as a member."""
    test_client.post("/spaces", json={"name": "members-list-ws"})
    response = test_client.get("/spaces/members-list-ws/members")
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_list_members_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-authz-ws"})
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
        response = test_client.get("/spaces/members-authz-ws/members")
    assert response.status_code == 403


def test_list_members_not_found_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 404 when space not found error occurs."""
    test_client.post("/spaces", json={"name": "members-notfound-ws"})
    with patch(
        "ugoite_core.list_members",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/members-notfound-ws/members")
    assert response.status_code == 404


def test_list_members_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-rt-ws"})
    with patch(
        "ugoite_core.list_members",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.get("/spaces/members-rt-ws/members")
    assert response.status_code == 500


def test_invite_member_already_active(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 409 when member is already active."""
    test_client.post("/spaces", json={"name": "members-dup-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("member already active")),
    ):
        response = test_client.post(
            "/spaces/members-dup-ws/members/invitations",
            json={"user_id": "alice", "role": "viewer"},
        )
    assert response.status_code == 409


def test_invite_member_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 404 when space/user not found."""
    test_client.post("/spaces", json={"name": "members-inv-404-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("user not found")),
    ):
        response = test_client.post(
            "/spaces/members-inv-404-ws/members/invitations",
            json={"user_id": "unknown-user", "role": "viewer"},
        )
    assert response.status_code == 404


def test_invite_member_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 400 on other runtime error."""
    test_client.post("/spaces", json={"name": "members-inv-rt-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("validation failed")),
    ):
        response = test_client.post(
            "/spaces/members-inv-rt-ws/members/invitations",
            json={"user_id": "alice-x", "role": "viewer"},
        )
    assert response.status_code == 400


def test_invite_member_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-inv-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/members-inv-authz-ws/members/invitations",
            json={"user_id": "alice", "role": "viewer"},
        )
    assert response.status_code == 403


def test_accept_invitation_expired(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 410 when invitation is expired."""
    test_client.post("/spaces", json={"name": "members-expired-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("invitation expired")),
    ):
        response = test_client.post(
            "/spaces/members-expired-ws/members/accept",
            json={"token": "expired-token"},
        )
    assert response.status_code == 410


def test_accept_invitation_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 404 when token not found."""
    test_client.post("/spaces", json={"name": "members-accept-404-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("invitation not found")),
    ):
        response = test_client.post(
            "/spaces/members-accept-404-ws/members/accept",
            json={"token": "bad-token"},
        )
    assert response.status_code == 404


def test_accept_invitation_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-accept-rt-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("bad request")),
    ):
        response = test_client.post(
            "/spaces/members-accept-rt-ws/members/accept",
            json={"token": "bad-token"},
        )
    assert response.status_code == 400


def test_update_member_role_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 404 when member does not exist."""
    test_client.post("/spaces", json={"name": "members-role-404-ws"})
    with patch(
        "ugoite_core.update_member_role",
        _amock(side_effect=RuntimeError("member not found")),
    ):
        response = test_client.post(
            "/spaces/members-role-404-ws/members/absent-user/role",
            json={"role": "editor"},
        )
    assert response.status_code == 404


def test_update_member_role_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-role-rt-ws"})
    with patch(
        "ugoite_core.update_member_role",
        _amock(side_effect=RuntimeError("invalid operation")),
    ):
        response = test_client.post(
            "/spaces/members-role-rt-ws/members/alice/role",
            json={"role": "editor"},
        )
    assert response.status_code == 400


def test_update_member_role_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-role-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/members-role-authz-ws/members/alice/role",
            json={"role": "editor"},
        )
    assert response.status_code == 403


def test_revoke_member_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 404 when member does not exist."""
    test_client.post("/spaces", json={"name": "members-revoke-404-ws"})
    with patch(
        "ugoite_core.revoke_member",
        _amock(side_effect=RuntimeError("member not found")),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-404-ws/members/absent-user",
        )
    assert response.status_code == 404


def test_revoke_member_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-revoke-rt-ws"})
    with patch(
        "ugoite_core.revoke_member",
        _amock(side_effect=RuntimeError("already revoked")),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-rt-ws/members/alice",
        )
    assert response.status_code == 400


def test_update_member_role_last_admin_conflict(test_client: TestClient) -> None:
    """REQ-SEC-006: role updates return 409 when removing the last admin."""
    test_client.post("/spaces", json={"name": "members-role-admin-conflict-ws"})
    with patch(
        "ugoite_core.update_member_role",
        _amock(side_effect=RuntimeError("space must retain at least one active admin")),
    ):
        response = test_client.post(
            "/spaces/members-role-admin-conflict-ws/members/alice/role",
            json={"role": "viewer"},
        )
    assert response.status_code == 409


def test_revoke_member_last_admin_conflict(test_client: TestClient) -> None:
    """REQ-SEC-006: revoke member returns 409 when it would remove the last admin."""
    test_client.post("/spaces", json={"name": "members-revoke-admin-conflict-ws"})
    with patch(
        "ugoite_core.revoke_member",
        _amock(side_effect=RuntimeError("space must retain at least one active admin")),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-admin-conflict-ws/members/alice",
        )
    assert response.status_code == 409


def test_revoke_member_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-revoke-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-authz-ws/members/alice",
        )
    assert response.status_code == 403
