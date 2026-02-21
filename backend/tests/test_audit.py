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
