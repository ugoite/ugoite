"""SQL session endpoints."""

import json
import logging
from collections.abc import AsyncGenerator
from typing import Annotated

import ugoite_core
from fastapi import APIRouter, HTTPException, Query, status
from fastapi.responses import StreamingResponse

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
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
) -> dict[str, object]:
    """Create a SQL session and execute the query."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.create_sql_session(
            storage_config,
            space_id,
            payload.sql,
        )
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
) -> dict[str, object]:
    """Get SQL session status."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_sql_session_status(
            storage_config,
            space_id,
            session_id,
        )
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
) -> dict[str, object]:
    """Get SQL session row count."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        count = await ugoite_core.get_sql_session_count(
            storage_config,
            space_id,
            session_id,
        )
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
    offset: Annotated[int, Query(ge=0)] = 0,
    limit: Annotated[int, Query(ge=1, le=500)] = 50,
) -> dict[str, object]:
    """Get paged SQL session rows."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_sql_session_rows(
            storage_config,
            space_id,
            session_id,
            offset,
            limit,
        )
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
) -> StreamingResponse:
    """Stream SQL session rows as NDJSON."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(session_id, "session_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    async def row_generator() -> AsyncGenerator[str]:
        rows = await ugoite_core.get_sql_session_rows_all(
            storage_config,
            space_id,
            session_id,
        )
        for row in rows:
            yield f"{json.dumps(row)}\n"

    return StreamingResponse(row_generator(), media_type="application/x-ndjson")
