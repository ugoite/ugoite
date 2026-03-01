"""Service account API tests.

REQ-SEC-009: Service Accounts and Scoped API Keys for Automation.
"""

from __future__ import annotations

from fastapi.testclient import TestClient

from app.main import app


def test_service_account_scopes_and_revocation_flow(test_client: TestClient) -> None:
    """REQ-SEC-009: scoped service keys enforce least privilege.

    Revoked keys are rejected immediately.
    """
    create_space = test_client.post("/spaces", json={"name": "svc-scope-space"})
    assert create_space.status_code == 201, create_space.text

    create_account = test_client.post(
        "/spaces/svc-scope-space/service-accounts",
        json={
            "display_name": "Read Bot",
            "scopes": ["entry_read"],
        },
    )
    assert create_account.status_code == 201, create_account.text
    account_id = create_account.json()["id"]

    create_key = test_client.post(
        f"/spaces/svc-scope-space/service-accounts/{account_id}/keys",
        json={"key_name": "ci-read"},
    )
    assert create_key.status_code == 201, create_key.text
    key_payload = create_key.json()
    key_id = key_payload["key"]["id"]
    secret = key_payload["secret"]

    service_client = TestClient(app, headers={"X-API-Key": secret})
    list_entries = service_client.get("/spaces/svc-scope-space/entries")
    assert list_entries.status_code == 200, list_entries.text

    denied_write = service_client.post(
        "/spaces/svc-scope-space/entries",
        json={"id": "e-1", "content": "# Entry\n\n## Form\nUser"},
    )
    assert denied_write.status_code == 403, denied_write.text
    assert "missing required scope" in denied_write.text

    revoke = test_client.delete(
        f"/spaces/svc-scope-space/service-accounts/{account_id}/keys/{key_id}",
    )
    assert revoke.status_code == 200, revoke.text

    rejected_after_revoke = service_client.get("/spaces/svc-scope-space/entries")
    assert rejected_after_revoke.status_code == 401, rejected_after_revoke.text
    assert "revoked" in rejected_after_revoke.text.lower()


from typing import Any
from unittest.mock import AsyncMock, patch
import ugoite_core


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_list_service_accounts_success(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns empty list for new space."""
    test_client.post("/spaces", json={"name": "sa-list-ws"})
    response = test_client.get("/spaces/sa-list-ws/service-accounts")
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_list_service_accounts_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-authz-ws"})
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
        response = test_client.get("/spaces/sa-authz-ws/service-accounts")
    assert response.status_code == 403


def test_list_service_accounts_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "sa-notfound-ws"})
    with patch(
        "ugoite_core.list_service_accounts",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/sa-notfound-ws/service-accounts")
    assert response.status_code == 404


def test_list_service_accounts_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-rt-ws"})
    with patch(
        "ugoite_core.list_service_accounts",
        _amock(side_effect=RuntimeError("validation failed")),
    ):
        response = test_client.get("/spaces/sa-rt-ws/service-accounts")
    assert response.status_code == 400


def test_create_service_account_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "sa-create-404-ws"})
    with patch(
        "ugoite_core.create_service_account",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.post(
            "/spaces/sa-create-404-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 404


def test_create_service_account_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-create-rt-ws"})
    with patch(
        "ugoite_core.create_service_account",
        _amock(side_effect=RuntimeError("invalid scope")),
    ):
        response = test_client.post(
            "/spaces/sa-create-rt-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 400


def test_create_service_account_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-create-authz-ws"})
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
            "/spaces/sa-create-authz-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 403


def test_create_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account key returns 404 when SA not found."""
    test_client.post("/spaces", json={"name": "sa-key-404-ws"})
    create_response = test_client.post(
        "/spaces/sa-key-404-ws/service-accounts",
        json={"display_name": "Bot", "scopes": ["entry_read"]},
    )
    sa_id = create_response.json()["id"]
    with patch(
        "ugoite_core.create_service_account_key",
        _amock(side_effect=RuntimeError("service account not found")),
    ):
        response = test_client.post(
            f"/spaces/sa-key-404-ws/service-accounts/{sa_id}/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 404


def test_create_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: create service account key returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-key-rt-ws"})
    create_response = test_client.post(
        "/spaces/sa-key-rt-ws/service-accounts",
        json={"display_name": "Bot", "scopes": ["entry_read"]},
    )
    sa_id = create_response.json()["id"]
    with patch(
        "ugoite_core.create_service_account_key",
        _amock(side_effect=RuntimeError("key limit exceeded")),
    ):
        response = test_client.post(
            f"/spaces/sa-key-rt-ws/service-accounts/{sa_id}/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 400


def test_create_service_account_key_authorization_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: create service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-key-create-authz-ws"})
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
            "/spaces/sa-key-create-authz-ws/service-accounts/sa-id/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 403


def test_rotate_service_account_key_full_flow(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns a new secret."""
    test_client.post("/spaces", json={"name": "sa-rotate-ws"})
    create_sa = test_client.post(
        "/spaces/sa-rotate-ws/service-accounts",
        json={"display_name": "RotateBot", "scopes": ["entry_read"]},
    )
    sa_id = create_sa.json()["id"]
    create_key = test_client.post(
        f"/spaces/sa-rotate-ws/service-accounts/{sa_id}/keys",
        json={"key_name": "original-key"},
    )
    key_id = create_key.json()["key"]["id"]

    response = test_client.post(
        f"/spaces/sa-rotate-ws/service-accounts/{sa_id}/keys/{key_id}/rotate",
        json={"key_name": "rotated-key"},
    )
    assert response.status_code == 201


def test_rotate_service_account_key_authz_error(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-rotate-authz-ws"})
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
            "/spaces/sa-rotate-authz-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 403


def test_rotate_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns 404 when key not found."""
    test_client.post("/spaces", json={"name": "sa-rotate-404-ws"})
    with patch(
        "ugoite_core.rotate_service_account_key",
        _amock(side_effect=RuntimeError("key not found")),
    ):
        response = test_client.post(
            "/spaces/sa-rotate-404-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 404


def test_rotate_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: rotate service account key returns 400 on generic error."""
    test_client.post("/spaces", json={"name": "sa-rotate-rt-ws"})
    with patch(
        "ugoite_core.rotate_service_account_key",
        _amock(side_effect=RuntimeError("invalid operation")),
    ):
        response = test_client.post(
            "/spaces/sa-rotate-rt-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 400


def test_revoke_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: revoke service account key returns 404 when key not found."""
    test_client.post("/spaces", json={"name": "sa-revoke-404-ws"})
    with patch(
        "ugoite_core.revoke_service_account_key",
        _amock(side_effect=RuntimeError("key not found")),
    ):
        response = test_client.delete(
            "/spaces/sa-revoke-404-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 404


def test_revoke_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: revoke service account key returns 400 on generic error."""
    test_client.post("/spaces", json={"name": "sa-revoke-rt-ws"})
    with patch(
        "ugoite_core.revoke_service_account_key",
        _amock(side_effect=RuntimeError("already revoked")),
    ):
        response = test_client.delete(
            "/spaces/sa-revoke-rt-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 400


def test_revoke_service_account_key_authorization_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: revoke service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-revoke-authz-ws"})
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
            "/spaces/sa-revoke-authz-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 403
