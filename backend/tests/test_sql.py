"""Saved SQL API tests.

REQ-API-006: Saved SQL CRUD.
"""

from fastapi.testclient import TestClient


def test_sql_req_api_006_crud(test_client: TestClient) -> None:
    """REQ-API-006: saved SQL CRUD works end-to-end."""
    response = test_client.post("/workspaces", json={"name": "sql-ws"})
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
        "/workspaces/sql-ws/sql",
        json=create_payload,
    )
    assert create_response.status_code == 201
    create_data = create_response.json()
    sql_id = create_data["id"]
    revision_id = create_data["revision_id"]
    assert revision_id

    get_response = test_client.get(f"/workspaces/sql-ws/sql/{sql_id}")
    assert get_response.status_code == 200
    get_data = get_response.json()
    assert get_data["name"] == "Recent Meetings"
    assert get_data["sql"].startswith("SELECT *")
    assert get_data["variables"][0]["name"] == "since"

    list_response = test_client.get("/workspaces/sql-ws/sql")
    assert list_response.status_code == 200
    assert any(item["id"] == sql_id for item in list_response.json())

    update_payload = {
        "name": "Recent Meetings",
        "sql": "SELECT * FROM Meeting WHERE Date >= {{since}}",
        "variables": create_payload["variables"],
        "parent_revision_id": revision_id,
    }
    update_response = test_client.put(
        f"/workspaces/sql-ws/sql/{sql_id}",
        json=update_payload,
    )
    assert update_response.status_code == 200
    update_data = update_response.json()
    assert update_data["revision_id"] != revision_id

    delete_response = test_client.delete(f"/workspaces/sql-ws/sql/{sql_id}")
    assert delete_response.status_code == 204

    missing_response = test_client.get(f"/workspaces/sql-ws/sql/{sql_id}")
    assert missing_response.status_code == 404


def test_sql_req_api_007_validation(test_client: TestClient) -> None:
    """REQ-API-007: saved SQL validates variables and SQL syntax."""
    response = test_client.post("/workspaces", json={"name": "sql-validate-ws"})
    assert response.status_code == 201

    missing_placeholder = {
        "name": "Missing placeholder",
        "sql": "SELECT * FROM Meeting",
        "variables": [
            {"type": "date", "name": "since", "description": "Lower bound"},
        ],
    }
    missing_response = test_client.post(
        "/workspaces/sql-validate-ws/sql",
        json=missing_placeholder,
    )
    assert missing_response.status_code == 422

    undefined_placeholder = {
        "name": "Undefined placeholder",
        "sql": "SELECT * FROM Meeting WHERE Date >= {{since}}",
        "variables": [],
    }
    undefined_response = test_client.post(
        "/workspaces/sql-validate-ws/sql",
        json=undefined_placeholder,
    )
    assert undefined_response.status_code == 422

    invalid_sql = {
        "name": "Invalid SQL",
        "sql": "FROM notes",
        "variables": [],
    }
    invalid_response = test_client.post(
        "/workspaces/sql-validate-ws/sql",
        json=invalid_sql,
    )
    assert invalid_response.status_code == 422
