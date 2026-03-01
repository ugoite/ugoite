"""Saved SQL API tests.

REQ-API-006: Saved SQL CRUD.
"""

from fastapi.testclient import TestClient


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

from typing import Any
from unittest.mock import AsyncMock, patch
import ugoite_core

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
        "ugoite_core.get_sql_session_count",
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
        "ugoite_core.get_sql_session_rows",
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

    page1 = {"rows": [{"id": "r1"}]}
    page2 = {"rows": []}

    call_count = {"n": 0}

    async def _rows_side(*args: object, **kwargs: object) -> dict[str, object]:
        call_count["n"] += 1
        return page1 if call_count["n"] == 1 else page2

    with patch("ugoite_core.get_sql_session_rows", AsyncMock(side_effect=_rows_side)):
        response = test_client.get(
            "/spaces/sess-stream-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert "r1" in response.text


def test_get_sql_session_stream_empty_rows(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns empty body when no rows."""
    test_client.post("/spaces", json={"name": "sess-stream-empty-ws"})
    with patch(
        "ugoite_core.get_sql_session_rows",
        _amock(return_value={"rows": []}),
    ):
        response = test_client.get(
            "/spaces/sess-stream-empty-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert response.text == ""
