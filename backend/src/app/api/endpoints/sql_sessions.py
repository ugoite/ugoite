"""SQL session endpoints."""

import json
import logging
from collections.abc import AsyncGenerator
from typing import Annotated

import ugoite_core
from fastapi import APIRouter, HTTPException, Query, Request, status
from fastapi.responses import StreamingResponse

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.core.authorization import (
    raise_authorization_http_error,
    request_identity,
)
from app.models.payloads import SqlSessionCreate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/spaces/{space_id}/sql-sessions",
    status_code=status.HTTP_201_CREATED,
)
async def create_sql_session_endpoint(
    space_id: str,
    payload: SqlSessionCreate,
    request: Request,
) -> dict[str, object]:
    """Create a SQL session and execute the query."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "sql_read",
        )
        return await ugoite_core.create_sql_session(
            storage_config,
            space_id,
            payload.sql,
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to create SQL session")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/sql-sessions/{session_id}")
async def get_sql_session_endpoint(
    space_id: str,
    session_id: str,
    request: Request,
) -> dict[str, object]:
    """Get SQL session status."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "sql_read",
        )
        return await ugoite_core.get_sql_session_status(
            storage_config,
            space_id,
            session_id,
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to load SQL session")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/sql-sessions/{session_id}/count")
async def get_sql_session_count_endpoint(
    space_id: str,
    session_id: str,
    request: Request,
) -> dict[str, object]:
    """Get SQL session row count."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "sql_read",
        )
        count = await ugoite_core.get_sql_session_count(
            storage_config,
            space_id,
            session_id,
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to load SQL session count")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"count": count}


@router.get("/spaces/{space_id}/sql-sessions/{session_id}/rows")
async def get_sql_session_rows_endpoint(
    space_id: str,
    session_id: str,
    request: Request,
    offset: Annotated[int, Query(ge=0)] = 0,
    limit: Annotated[int, Query(ge=1, le=500)] = 50,
) -> dict[str, object]:
    """Get paged SQL session rows."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "sql_read",
        )
        return await ugoite_core.get_sql_session_rows(
            storage_config,
            space_id,
            session_id,
            offset,
            limit,
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to load SQL session rows")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/sql-sessions/{session_id}/stream")
async def get_sql_session_stream_endpoint(
    space_id: str,
    session_id: str,
    request: Request,
) -> StreamingResponse:
    """Stream SQL session rows as NDJSON."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "sql_read",
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)

    async def row_generator() -> AsyncGenerator[str]:
        offset = 0
        page_size = 200
        while True:
            page = await ugoite_core.get_sql_session_rows(
                storage_config,
                space_id,
                session_id,
                offset,
                page_size,
            )
            rows_obj = page.get("rows") if isinstance(page, dict) else None
            rows = rows_obj if isinstance(rows_obj, list) else []
            if not rows:
                break

            for row in rows:
                yield f"{json.dumps(row)}\n"

            offset += len(rows)
            if len(rows) < page_size:
                break

    return StreamingResponse(row_generator(), media_type="application/x-ndjson")
