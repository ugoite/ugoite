"""Saved SQL API tests.

REQ-API-006: Saved SQL CRUD.
REQ-API-008: SQL session query API.
"""

import asyncio
import json
from collections.abc import Iterator
from pathlib import Path
from typing import Any
from unittest.mock import AsyncMock, patch

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


def _invite_and_accept_viewer(
    owner_client: TestClient,
    viewer_client: TestClient,
    space_id: str,
) -> None:
    invite_response = owner_client.post(
        f"/spaces/{space_id}/members/invitations",
        json={"user_id": "viewer-user", "role": "viewer"},
    )
    assert invite_response.status_code == 201

    accept_response = viewer_client.post(
        f"/spaces/{space_id}/members/accept",
        json={"token": invite_response.json()["invitation"]["token"]},
    )
    assert accept_response.status_code == 200


@pytest.fixture
def sql_auth_clients(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> Iterator[dict[str, TestClient]]:
    """Provide deterministic multi-user clients for SQL ACL tests."""
    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_TOKENS_JSON",
        json.dumps(
            {
                "owner-token": {"user_id": "owner-user", "principal_type": "user"},
                "viewer-token": {"user_id": "viewer-user", "principal_type": "user"},
            },
        ),
    )
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    _bootstrap_admin_space_for_user(temp_space_root, "owner-user")
    clear_auth_manager_cache()

    yield {
        "owner": TestClient(app, headers={"Authorization": "Bearer owner-token"}),
        "viewer": TestClient(app, headers={"Authorization": "Bearer viewer-token"}),
    }

    clear_auth_manager_cache()


def test_sql_req_api_006_crud(test_client: TestClient) -> None:
    """REQ-API-006: saved SQL CRUD works end-to-end."""
    response = test_client.post("/spaces", json={"name": "sql-ws"})
    assert response.status_code == 201

    create_payload = {
        "name": "Recent Meetings",
        "sql": (
            "SELECT * FROM Meeting WHERE Date >= {{since}} "
            "ORDER BY updated_at DESC LIMIT 50"
        ),
        "variables": [
            {
                "type": "date",
                "name": "since",
                "description": "Lower bound date",
            },
        ],
    }

    create_response = test_client.post(
        "/spaces/sql-ws/sql",
        json=create_payload,
    )
    assert create_response.status_code == 201
    create_data = create_response.json()
    sql_id = create_data["id"]
    revision_id = create_data["revision_id"]
    assert revision_id

    get_response = test_client.get(f"/spaces/sql-ws/sql/{sql_id}")
    assert get_response.status_code == 200
    get_data = get_response.json()
    assert get_data["name"] == "Recent Meetings"
    assert get_data["sql"].startswith("SELECT *")
    assert get_data["variables"][0]["name"] == "since"

    list_response = test_client.get("/spaces/sql-ws/sql")
    assert list_response.status_code == 200
    assert any(item["id"] == sql_id for item in list_response.json())

    update_payload = {
        "name": "Recent Meetings",
        "sql": "SELECT * FROM Meeting WHERE Date >= {{since}}",
        "variables": create_payload["variables"],
        "parent_revision_id": revision_id,
    }
    update_response = test_client.put(
        f"/spaces/sql-ws/sql/{sql_id}",
        json=update_payload,
    )
    assert update_response.status_code == 200
    update_data = update_response.json()
    assert update_data["revision_id"] != revision_id

    delete_response = test_client.delete(f"/spaces/sql-ws/sql/{sql_id}")
    assert delete_response.status_code == 204

    missing_response = test_client.get(f"/spaces/sql-ws/sql/{sql_id}")
    assert missing_response.status_code == 404


def test_sql_req_api_007_validation(test_client: TestClient) -> None:
    """REQ-API-007: saved SQL validates variables and SQL syntax."""
    response = test_client.post("/spaces", json={"name": "sql-validate-ws"})
    assert response.status_code == 201

    missing_placeholder = {
        "name": "Missing placeholder",
        "sql": "SELECT * FROM Meeting",
        "variables": [
            {"type": "date", "name": "since", "description": "Lower bound"},
        ],
    }
    missing_response = test_client.post(
        "/spaces/sql-validate-ws/sql",
        json=missing_placeholder,
    )
    assert missing_response.status_code == 422

    undefined_placeholder = {
        "name": "Undefined placeholder",
        "sql": "SELECT * FROM Meeting WHERE Date >= {{since}}",
        "variables": [],
    }
    undefined_response = test_client.post(
        "/spaces/sql-validate-ws/sql",
        json=undefined_placeholder,
    )
    assert undefined_response.status_code == 422

    invalid_sql = {
        "name": "Invalid SQL",
        "sql": "FROM entries",
        "variables": [],
    }
    invalid_response = test_client.post(
        "/spaces/sql-validate-ws/sql",
        json=invalid_sql,
    )
    assert invalid_response.status_code == 422


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


def test_list_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: list SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/sql-authz-ws/sql")
    assert response.status_code == 403


def test_list_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: list SQL returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "sql-list-err-ws"})
    with patch(
        "ugoite_core.list_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/sql-list-err-ws/sql")
    assert response.status_code == 500


def test_create_sql_already_exists(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 409 when entry already exists."""
    test_client.post("/spaces", json={"name": "sql-dup-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=RuntimeError("sql entry already exists")),
    ):
        response = test_client.post(
            "/spaces/sql-dup-ws/sql",
            json={"name": "Dup", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 409


def test_create_sql_validation_error(test_client: TestClient) -> None:
    """REQ-API-007: create SQL returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sql-val-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(
            side_effect=RuntimeError("UGOITE_SQL_VALIDATION: reserved keyword"),
        ),
    ):
        response = test_client.post(
            "/spaces/sql-val-ws/sql",
            json={"name": "Bad", "sql": "SELECT SYSTEM", "variables": []},
        )
    assert response.status_code == 422


def test_create_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-create-rt-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/sql-create-rt-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 500


def test_create_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-create-exc-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/sql-create-exc-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 500


def test_create_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-create-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sql-create-authz-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 403


def test_get_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-get-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/sql-get-authz-ws/sql/some-id")
    assert response.status_code == 403


def test_get_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-get-exc-ws"})
    with patch(
        "ugoite_core.get_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/sql-get-exc-ws/sql/some-id")
    assert response.status_code == 500


def test_get_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-get-rt-ws"})
    with patch(
        "ugoite_core.get_sql",
        _amock(side_effect=RuntimeError("storage corruption")),
    ):
        response = test_client.get("/spaces/sql-get-rt-ws/sql/some-id")
    assert response.status_code == 500


def test_update_sql_conflict(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 409 on revision conflict."""
    test_client.post("/spaces", json={"name": "sql-upd-conflict-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("conflict detected")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-conflict-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "old-rev",
            },
        )
    assert response.status_code == 409


def test_update_sql_not_found(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 404 when entry not found."""
    test_client.post("/spaces", json={"name": "sql-upd-404-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("entry not found")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-404-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 404


def test_update_sql_validation_error(test_client: TestClient) -> None:
    """REQ-API-007: update SQL returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sql-upd-val-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("UGOITE_SQL_VALIDATION: bad syntax")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-val-ws/sql/some-sql-id",
            json={
                "name": "Bad",
                "sql": "INVALID SQL",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 422


def test_update_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-upd-rt-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-rt-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 500


def test_update_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-upd-exc-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-exc-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 500


def test_update_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-upd-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.put(
            "/spaces/sql-upd-authz-ws/sql/some-id",
            json={
                "name": "Test",
                "sql": "SELECT 1",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 403


def test_delete_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-del-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.delete("/spaces/sql-del-authz-ws/sql/some-sql-id")
    assert response.status_code == 403


def test_delete_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-del-rt-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.delete("/spaces/sql-del-rt-ws/sql/some-sql-id")
    assert response.status_code == 500


def test_delete_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-del-exc-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.delete("/spaces/sql-del-exc-ws/sql/some-sql-id")
    assert response.status_code == 500


def test_delete_sql_not_found(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 404 when SQL entry not found."""
    test_client.post("/spaces", json={"name": "sql-del-404-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=RuntimeError("sql entry not found")),
    ):
        response = test_client.delete("/spaces/sql-del-404-ws/sql/some-id")
    assert response.status_code == 404


def test_create_sql_session_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sess-authz-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 403


def test_create_sql_session_validation_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sess-val-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(
            side_effect=RuntimeError(
                "UGOITE_SQL_VALIDATION: missing placeholder",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sess-val-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 422


def test_create_sql_session_rejects_oversized_sql(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session rejects oversized SQL payloads before core."""
    with patch("ugoite_core.create_sql_session", _amock()) as create_sql_session:
        response = test_client.post(
            "/spaces/sess-too-large-ws/sql-sessions",
            json={"sql": "S" * 100_001},
        )
    assert response.status_code == 422
    assert any(
        item["loc"][-1] == "sql" and item["type"] == "string_too_long"
        for item in response.json()["detail"]
    )
    create_sql_session.assert_not_awaited()


def test_create_sql_session_accepts_sql_at_max_length(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session accepts SQL payloads at the max boundary."""
    test_client.post("/spaces", json={"name": "sess-max-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(return_value={"id": "sess-max"}),
    ) as create_sql_session:
        response = test_client.post(
            "/spaces/sess-max-ws/sql-sessions",
            json={"sql": "S" * 100_000},
        )
    assert response.status_code == 201
    create_sql_session.assert_awaited_once()


def test_create_sql_session_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sess-rt-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/sess-rt-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 500


def test_create_sql_session_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sess-exc-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/sess-exc-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 500


def test_get_sql_session_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-get-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-get-authz-ws/sql-sessions/some-session",
        )
    assert response.status_code == 403


def test_get_sql_session_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-get-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_status",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-get-exc-ws/sql-sessions/some-session",
        )
    assert response.status_code == 500


def test_get_sql_session_count_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session count returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-cnt-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-cnt-authz-ws/sql-sessions/some-session/count",
        )
    assert response.status_code == 403


def test_get_sql_session_count_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session count returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-cnt-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_count_for_identity",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-cnt-exc-ws/sql-sessions/some-session/count",
        )
    assert response.status_code == 500


def test_get_sql_session_rows_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session rows returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-rows-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-rows-authz-ws/sql-sessions/some-session/rows",
        )
    assert response.status_code == 403


def test_get_sql_session_rows_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session rows returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-rows-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_rows_for_identity",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-rows-exc-ws/sql-sessions/some-session/rows",
        )
    assert response.status_code == 500


def test_get_sql_session_stream_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-stream-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-stream-authz-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 403


def test_get_sql_session_stream_success(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns NDJSON rows."""
    test_client.post("/spaces", json={"name": "sess-stream-ws"})

    page1: dict[str, object] = {"rows": [{"id": "r1"}]}
    page2: dict[str, object] = {"rows": []}

    call_count = {"n": 0}

    async def _rows_side(*args: object, **kwargs: object) -> dict[str, object]:
        call_count["n"] += 1
        return page1 if call_count["n"] == 1 else page2

    with patch(
        "ugoite_core.get_sql_session_rows_for_identity",
        AsyncMock(side_effect=_rows_side),
    ):
        response = test_client.get(
            "/spaces/sess-stream-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert "r1" in response.text


def test_get_sql_session_stream_empty_rows(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns empty body when no rows."""
    test_client.post("/spaces", json={"name": "sess-stream-empty-ws"})
    with patch(
        "ugoite_core.get_sql_session_rows_for_identity",
        _amock(return_value={"rows": []}),
    ):
        response = test_client.get(
            "/spaces/sess-stream-empty-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert response.text == ""


def test_sql_sessions_req_api_008_filter_form_acl_before_limit(
    sql_auth_clients: dict[str, TestClient],
) -> None:
    """REQ-API-008: SQL sessions apply form ACLs before ORDER BY/LIMIT evaluation."""
    owner = sql_auth_clients["owner"]
    viewer = sql_auth_clients["viewer"]

    create_space = owner.post("/spaces", json={"name": "sql-acl-ws"})
    assert create_space.status_code == 201
    space_id = create_space.json()["id"]
    _invite_and_accept_viewer(owner, viewer, space_id)

    public_form = owner.post(
        f"/spaces/{space_id}/forms",
        json={
            "name": "PublicTask",
            "version": 1,
            "template": "# PublicTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string", "required": True}},
        },
    )
    assert public_form.status_code == 201

    restricted_form = owner.post(
        f"/spaces/{space_id}/forms",
        json={
            "name": "RestrictedTask",
            "version": 1,
            "template": "# RestrictedTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string", "required": True}},
            "read_principals": [{"kind": "user", "id": "owner-user"}],
            "write_principals": [{"kind": "user", "id": "owner-user"}],
        },
    )
    assert restricted_form.status_code == 201

    public_a = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "id": "public-a",
            "content": "---\nform: PublicTask\n---\n## Summary\nPublic A\n",
        },
    )
    assert public_a.status_code == 201

    public_b = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "id": "public-b",
            "content": "---\nform: PublicTask\n---\n## Summary\nPublic B\n",
        },
    )
    assert public_b.status_code == 201

    restricted = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "id": "restricted-z",
            "content": "---\nform: RestrictedTask\n---\n## Summary\nRestricted Z\n",
        },
    )
    assert restricted.status_code == 201

    viewer_get = viewer.get(f"/spaces/{space_id}/entries/restricted-z")
    assert viewer_get.status_code == 403

    create_session = viewer.post(
        f"/spaces/{space_id}/sql-sessions",
        json={"sql": "SELECT * FROM entries ORDER BY id DESC LIMIT 2"},
    )
    assert create_session.status_code == 201
    session_id = create_session.json()["id"]

    count_response = viewer.get(f"/spaces/{space_id}/sql-sessions/{session_id}/count")
    assert count_response.status_code == 200
    assert count_response.json() == {"count": 2}

    rows_response = viewer.get(
        f"/spaces/{space_id}/sql-sessions/{session_id}/rows",
        params={"offset": 0, "limit": 10},
    )
    assert rows_response.status_code == 200
    rows_payload = rows_response.json()
    assert rows_payload["total_count"] == 2
    assert [row["id"] for row in rows_payload["rows"]] == ["public-b", "public-a"]


def test_sql_sessions_req_api_008_filter_auxiliary_tables_by_acl(
    sql_auth_clients: dict[str, TestClient],
) -> None:
    """REQ-API-008: SQL sessions scope auxiliary assets tables by readable entries."""
    owner = sql_auth_clients["owner"]
    viewer = sql_auth_clients["viewer"]

    create_space = owner.post("/spaces", json={"name": "sql-aux-ws"})
    assert create_space.status_code == 201
    space_id = create_space.json()["id"]
    _invite_and_accept_viewer(owner, viewer, space_id)

    assert (
        owner.post(
            f"/spaces/{space_id}/forms",
            json={
                "name": "PublicTask",
                "version": 1,
                "template": "# PublicTask\n\n## Summary\n",
                "fields": {"Summary": {"type": "string", "required": True}},
            },
        ).status_code
        == 201
    )
    assert (
        owner.post(
            f"/spaces/{space_id}/forms",
            json={
                "name": "RestrictedTask",
                "version": 1,
                "template": "# RestrictedTask\n\n## Summary\n",
                "fields": {"Summary": {"type": "string", "required": True}},
                "read_principals": [{"kind": "user", "id": "owner-user"}],
                "write_principals": [{"kind": "user", "id": "owner-user"}],
            },
        ).status_code
        == 201
    )

    public_a = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "id": "public-a",
            "content": (
                "---\nform: PublicTask\n---\n# Public A\n\n## Summary\nPublic A\n"
            ),
        },
    )
    assert public_a.status_code == 201
    public_a_revision = public_a.json()["revision_id"]

    assert (
        owner.post(
            f"/spaces/{space_id}/entries",
            json={
                "id": "public-b",
                "content": "---\nform: PublicTask\n---\n# Public B\n\n## Summary\nPublic B\n",
            },
        ).status_code
        == 201
    )

    restricted = owner.post(
        f"/spaces/{space_id}/entries",
        json={
            "id": "restricted-z",
            "content": (
                "---\nform: RestrictedTask\n---\n"
                "# Restricted Z\n\n## Summary\nRestricted Z\n"
            ),
        },
    )
    assert restricted.status_code == 201
    restricted_revision = restricted.json()["revision_id"]

    public_asset = owner.post(
        f"/spaces/{space_id}/assets",
        files={"file": ("public.txt", b"public asset", "text/plain")},
    )
    assert public_asset.status_code == 201
    restricted_asset = owner.post(
        f"/spaces/{space_id}/assets",
        files={"file": ("restricted.txt", b"restricted asset", "text/plain")},
    )
    assert restricted_asset.status_code == 201

    assert (
        owner.put(
            f"/spaces/{space_id}/entries/public-a",
            json={
                "markdown": (
                    "---\nform: PublicTask\n---\n# Public A\n\n## Summary\nPublic A\n"
                ),
                "parent_revision_id": public_a_revision,
                "assets": [public_asset.json()],
            },
        ).status_code
        == 200
    )
    assert (
        owner.put(
            f"/spaces/{space_id}/entries/restricted-z",
            json={
                "markdown": (
                    "---\nform: RestrictedTask\n---\n# Restricted Z\n\n## Summary\nRestricted Z\n"
                ),
                "parent_revision_id": restricted_revision,
                "assets": [restricted_asset.json()],
            },
        ).status_code
        == 200
    )

    assets_session = viewer.post(
        f"/spaces/{space_id}/sql-sessions",
        json={"sql": "SELECT * FROM assets ORDER BY entry_id"},
    )
    assert assets_session.status_code == 201
    assets_rows = viewer.get(
        f"/spaces/{space_id}/sql-sessions/{assets_session.json()['id']}/rows",
        params={"offset": 0, "limit": 10},
    )
    assert assets_rows.status_code == 200
    assert assets_rows.json()["total_count"] == 1
    asset_rows = assets_rows.json()["rows"]
    assert len(asset_rows) == 1
    assert asset_rows[0]["entry_id"] == "public-a"
    assert asset_rows[0]["name"] == "public.txt"
