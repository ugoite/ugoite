"""Authorization-aware SQL session helpers.

REQ-API-008: SQL session query API.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, cast

from . import _ugoite_core as _core
from .authz import AuthorizationError, require_form_read, require_space_action

if TYPE_CHECKING:
    from .auth import RequestIdentity

_core_any = cast("Any", _core)


@dataclass(frozen=True)
class SqlSessionPageInput:
    """Pagination window for SQL session row queries."""

    offset: int
    limit: int


async def _resolve_sql_read_scope(
    storage_config: dict[str, Any],
    space_id: str,
    identity: RequestIdentity,
) -> tuple[list[str], bool]:
    readable_forms: list[str] = []
    for form in await _core_any.list_forms(storage_config, space_id):
        if not isinstance(form, dict):
            continue
        form_name = form.get("name")
        if not isinstance(form_name, str) or not form_name:
            continue
        try:
            await require_form_read(storage_config, space_id, identity, form_name)
        except AuthorizationError:
            continue
        readable_forms.append(form_name)

    try:
        await require_space_action(storage_config, space_id, identity, "entry_read")
    except AuthorizationError:
        include_untyped_entries = False
    else:
        include_untyped_entries = True

    return readable_forms, include_untyped_entries


async def get_sql_session_count_for_identity(
    storage_config: dict[str, Any],
    space_id: str,
    identity: RequestIdentity,
    session_id: str,
) -> int:
    """REQ-API-008: count SQL rows using the caller's readable entry scope."""
    readable_forms, include_untyped_entries = await _resolve_sql_read_scope(
        storage_config,
        space_id,
        identity,
    )
    return cast(
        "int",
        await _core_any.get_sql_session_count_scoped(
            storage_config,
            space_id,
            session_id,
            readable_forms,
            include_untyped_entries,
        ),
    )


async def get_sql_session_rows_for_identity(
    storage_config: dict[str, Any],
    space_id: str,
    identity: RequestIdentity,
    session_id: str,
    page: SqlSessionPageInput,
) -> dict[str, object]:
    """REQ-API-008: page SQL rows using the caller's readable entry scope."""
    readable_forms, include_untyped_entries = await _resolve_sql_read_scope(
        storage_config,
        space_id,
        identity,
    )
    return cast(
        "dict[str, object]",
        await _core_any.get_sql_session_rows_scoped(
            storage_config,
            space_id,
            session_id,
            page.offset,
            page.limit,
            readable_forms,
            include_untyped_entries,
        ),
    )


async def get_sql_session_rows_all_for_identity(
    storage_config: dict[str, Any],
    space_id: str,
    identity: RequestIdentity,
    session_id: str,
) -> list[dict[str, object]]:
    """REQ-API-008: read all SQL rows using the caller's readable entry scope."""
    readable_forms, include_untyped_entries = await _resolve_sql_read_scope(
        storage_config,
        space_id,
        identity,
    )
    return cast(
        "list[dict[str, object]]",
        await _core_any.get_sql_session_rows_all_scoped(
            storage_config,
            space_id,
            session_id,
            readable_forms,
            include_untyped_entries,
        ),
    )


__all__ = [
    "SqlSessionPageInput",
    "get_sql_session_count_for_identity",
    "get_sql_session_rows_all_for_identity",
    "get_sql_session_rows_for_identity",
]
