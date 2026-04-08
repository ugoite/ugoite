"""SQL session authorization tests.

REQ-API-008: SQL session query API.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, cast

import pytest

import ugoite_core
from ugoite_core.auth import RequestIdentity

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.asyncio
async def test_sql_sessions_req_api_008_filters_form_acl_before_limit(
    tmp_path: Path,
) -> None:
    """REQ-API-008: SQL sessions filter unreadable Forms before ORDER BY/LIMIT."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    space_id = "sql-acl-space"

    await ugoite_core.create_space(config, space_id)

    await ugoite_core.upsert_form(
        config,
        space_id,
        json.dumps(
            {
                "name": "PublicTask",
                "version": 1,
                "template": "# PublicTask\n\n## Summary\n",
                "fields": {"Summary": {"type": "string", "required": True}},
            },
        ),
    )
    await ugoite_core.upsert_form(
        config,
        space_id,
        json.dumps(
            {
                "name": "RestrictedTask",
                "version": 1,
                "template": "# RestrictedTask\n\n## Summary\n",
                "fields": {"Summary": {"type": "string", "required": True}},
            },
        ),
    )
    await ugoite_core.patch_space(
        config,
        space_id,
        json.dumps(
            {
                "settings": {
                    "member_roles": {
                        "owner-user": "owner",
                        "viewer-user": "viewer",
                    },
                    "form_acls": {
                        "RestrictedTask": {
                            "read_principals": [
                                {"kind": "user", "id": "owner-user"},
                            ],
                            "write_principals": [
                                {"kind": "user", "id": "owner-user"},
                            ],
                        },
                    },
                },
            },
        ),
    )

    await ugoite_core.create_entry(
        config,
        space_id,
        "public-a",
        "---\nform: PublicTask\n---\n# Public A\n\n## Summary\nPublic A\n",
        author="owner-user",
    )
    await ugoite_core.create_entry(
        config,
        space_id,
        "public-b",
        "---\nform: PublicTask\n---\n# Public B\n\n## Summary\nPublic B\n",
        author="owner-user",
    )
    await ugoite_core.create_entry(
        config,
        space_id,
        "restricted-z",
        (
            "---\nform: RestrictedTask\n---\n"
            "# Restricted Z\n\n## Summary\nRestricted Z\n"
        ),
        author="owner-user",
    )

    session = await ugoite_core.create_sql_session(
        config,
        space_id,
        "SELECT * FROM entries ORDER BY id DESC LIMIT 2",
    )
    session_id = session["id"]
    viewer = RequestIdentity(user_id="viewer-user", auth_method="bearer")

    count = await ugoite_core.get_sql_session_count_for_identity(
        config,
        space_id,
        viewer,
        session_id,
    )
    assert count == 2

    rows = await ugoite_core.get_sql_session_rows_for_identity(
        config,
        space_id,
        viewer,
        session_id,
        ugoite_core.SqlSessionPageInput(offset=0, limit=10),
    )
    assert rows["total_count"] == 2
    row_items = cast("list[dict[str, object]]", rows["rows"])
    assert [row["id"] for row in row_items] == ["public-b", "public-a"]

    all_rows = await ugoite_core.get_sql_session_rows_all_for_identity(
        config,
        space_id,
        viewer,
        session_id,
    )
    assert [row["id"] for row in all_rows] == ["public-b", "public-a"]
