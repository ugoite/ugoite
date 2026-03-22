"""Reserved identity Form tests.

REQ-FORM-008: Reserved Identity Metadata Forms.
"""

import uuid

from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app, headers={"Authorization": "Bearer test-suite-token"})


def _space_id() -> str:
    response = client.post("/spaces", json={"name": f"reserved-id-{uuid.uuid4().hex}"})
    assert response.status_code == 201
    return response.json()["id"]


def test_form_req_form_008_reject_user_form_name() -> None:
    """REQ-FORM-008: user-authored Form name `User` is rejected."""
    response = client.post(
        f"/spaces/{_space_id()}/forms",
        json={
            "name": "User",
            "version": 1,
            "template": "# User\n\n## DisplayName\n",
            "fields": {"DisplayName": {"type": "string"}},
        },
    )

    assert response.status_code == 422
    assert "reserved" in response.json()["detail"].lower()


def test_form_req_form_008_reject_usergroup_form_name() -> None:
    """REQ-FORM-008: user-authored Form name `UserGroup` is rejected."""
    response = client.post(
        f"/spaces/{_space_id()}/forms",
        json={
            "name": "UserGroup",
            "version": 1,
            "template": "# UserGroup\n\n## Name\n",
            "fields": {"Name": {"type": "string"}},
        },
    )

    assert response.status_code == 422
    assert "reserved" in response.json()["detail"].lower()
