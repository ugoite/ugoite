"""Tests for saved SQL helpers.

REQ-API-006: Saved SQL CRUD.
REQ-API-007: Saved SQL variable embedding and validity.
"""

from pathlib import Path

import pytest

from ieapp.saved_sql import create_sql, delete_sql, get_sql, list_sql, update_sql
from ieapp.space import create_space


def _space_path(tmp_path: Path) -> str:
    create_space(str(tmp_path), "ws")
    return str(tmp_path / "spaces" / "ws")


def test_saved_sql_req_api_006_crud(tmp_path: Path) -> None:
    """REQ-API-006: saved SQL CRUD works in the CLI layer."""
    ws_path = _space_path(tmp_path)

    payload = {
        "name": "Recent Meetings",
        "sql": "SELECT * FROM entries WHERE updated_at >= {{since}}",
        "variables": [
            {
                "type": "date",
                "name": "since",
                "description": "Lower bound",
            },
        ],
    }

    entry = create_sql(ws_path, payload)
    sql_id = entry["id"]
    revision_id = entry["revision_id"]

    fetched = get_sql(ws_path, sql_id)
    assert fetched["name"] == "Recent Meetings"

    entries = list_sql(ws_path)
    assert any(item["id"] == sql_id for item in entries)

    update_payload = {
        "name": "Recent Meetings",
        "sql": (
            "SELECT * FROM entries WHERE updated_at >= {{since}} "
            "ORDER BY updated_at DESC"
        ),
        "variables": payload["variables"],
        "parent_revision_id": revision_id,
    }

    updated = update_sql(ws_path, sql_id, update_payload)
    assert updated["revision_id"] != revision_id

    delete_sql(ws_path, sql_id)
    with pytest.raises(RuntimeError, match="not found"):
        get_sql(ws_path, sql_id)


def test_saved_sql_req_api_007_validation(tmp_path: Path) -> None:
    """REQ-API-007: saved SQL validation errors surface in the CLI layer."""
    ws_path = _space_path(tmp_path)

    invalid_payload = {
        "name": "Missing placeholder",
        "sql": "SELECT * FROM entries",
        "variables": [
            {
                "type": "date",
                "name": "since",
                "description": "Lower bound",
            },
        ],
    }

    with pytest.raises(RuntimeError, match="IEAPP_SQL_VALIDATION"):
        create_sql(ws_path, invalid_payload)
