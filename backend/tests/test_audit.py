"""Audit logging API tests.

REQ-SEC-008: Security Audit Logging and Attribution.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache

if TYPE_CHECKING:
    import pytest


def test_audit_lists_data_mutation_events(test_client: TestClient) -> None:
    """REQ-SEC-008: data mutation operations are persisted as audit events."""
    create_space = test_client.post("/spaces", json={"name": "audit-space"})
    assert create_space.status_code == 201

    patch_space = test_client.patch(
        "/spaces/audit-space",
        json={"settings": {"audit_marker": "enabled"}},
    )
    assert patch_space.status_code == 200

    response = test_client.get("/spaces/audit-space/audit/events")
    assert response.status_code == 200
    payload = response.json()
    assert payload["total"] >= 1
    assert any(item["action"] == "data.mutation" for item in payload["items"])


def test_audit_logs_authorization_denial(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-008: authorization denials are persisted in audit logs."""
    create_space = test_client.post("/spaces", json={"name": "audit-space"})
    assert create_space.status_code == 201

    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_TOKENS_JSON",
        json.dumps(
            {
                "test-suite-token": {
                    "user_id": "test-suite-user",
                    "principal_type": "user",
                    "key_id": "owner-token",
                },
                "intruder-token": {
                    "user_id": "intruder-user",
                    "principal_type": "user",
                    "key_id": "intruder-token",
                },
            },
        ),
    )
    clear_auth_manager_cache()

    intruder = TestClient(
        app=test_client.app,
        headers={"Authorization": "Bearer intruder-token"},
    )
    denied = intruder.get("/spaces/audit-space/entries")
    assert denied.status_code == 403

    owner = TestClient(
        app=test_client.app,
        headers={"Authorization": "Bearer test-suite-token"},
    )
    audit = owner.get("/spaces/audit-space/audit/events?outcome=deny")
    assert audit.status_code == 200
    payload = audit.json()
    assert payload["total"] >= 1
    assert any(item.get("outcome") == "deny" for item in payload["items"])


from typing import Any
from unittest.mock import AsyncMock, patch

import ugoite_core


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_list_audit_events_integrity_error(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 409 on integrity chain error."""
    test_client.post("/spaces", json={"name": "audit-chain-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("integrity chain violation")),
    ):
        response = test_client.get("/spaces/audit-chain-ws/audit/events")
    assert response.status_code == 409


def test_list_audit_events_not_found(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "audit-notfound-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/audit-notfound-ws/audit/events")
    assert response.status_code == 404


def test_list_audit_events_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "audit-rterr-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("unexpected storage error")),
    ):
        response = test_client.get("/spaces/audit-rterr-ws/audit/events")
    # "unexpected storage error" does not contain "integrity", "chain", or "not found"
    assert response.status_code == 500


def test_list_audit_events_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-008: list audit events returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "audit-authz-ws"})
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
        response = test_client.get("/spaces/audit-authz-ws/audit/events")
    assert response.status_code == 403
