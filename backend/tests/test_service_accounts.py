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
