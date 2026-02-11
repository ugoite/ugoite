"""Saved SQL endpoints."""

import json
import logging
import uuid
from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.models.payloads import SqlCreate, SqlUpdate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.get("/spaces/{space_id}/sql")
async def list_sql_endpoint(space_id: str) -> list[dict[str, Any]]:
    """List all saved SQL entries in a space."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.list_sql(storage_config, space_id)
    except Exception as e:
        logger.exception("Failed to list saved SQL")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/spaces/{space_id}/sql", status_code=status.HTTP_201_CREATED)
async def create_sql_endpoint(
    space_id: str,
    payload: SqlCreate,
) -> dict[str, Any]:
    """Create a new saved SQL entry."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    if payload.id:
        _validate_path_id(payload.id, "sql_id")
    sql_id = payload.id or str(uuid.uuid4())

    try:
        payload_json = json.dumps(
            {
                "name": payload.name,
                "sql": payload.sql,
                "variables": [var.model_dump() for var in payload.variables],
            },
        )
        entry = await ugoite_core.create_sql(
            storage_config,
            space_id,
            sql_id,
            payload_json,
        )
        return {"id": sql_id, "revision_id": entry.get("revision_id", "")}
    except RuntimeError as e:
        msg = str(e)
        if "already exists" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=msg,
            ) from e
        if msg.startswith("UGOITE_SQL_VALIDATION") or "reserved" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to create saved SQL")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/sql/{sql_id}")
async def get_sql_endpoint(space_id: str, sql_id: str) -> dict[str, Any]:
    """Get a saved SQL entry by ID."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(sql_id, "sql_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_sql(storage_config, space_id, sql_id)
    except RuntimeError as e:
        msg = str(e)
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to get saved SQL")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.put("/spaces/{space_id}/sql/{sql_id}")
async def update_sql_endpoint(
    space_id: str,
    sql_id: str,
    payload: SqlUpdate,
) -> dict[str, Any]:
    """Update a saved SQL entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(sql_id, "sql_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        payload_json = json.dumps(
            {
                "name": payload.name,
                "sql": payload.sql,
                "variables": [var.model_dump() for var in payload.variables],
            },
        )
        entry = await ugoite_core.update_sql(
            storage_config,
            space_id,
            sql_id,
            payload_json,
            parent_revision_id=payload.parent_revision_id,
        )
        return {"id": sql_id, "revision_id": entry.get("revision_id", "")}
    except RuntimeError as e:
        msg = str(e)
        if "conflict" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=msg,
            ) from e
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=msg,
            ) from e
        if msg.startswith("UGOITE_SQL_VALIDATION"):
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to update saved SQL")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete(
    "/spaces/{space_id}/sql/{sql_id}",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def delete_sql_endpoint(space_id: str, sql_id: str) -> None:
    """Delete a saved SQL entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(sql_id, "sql_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.delete_sql(storage_config, space_id, sql_id)
    except RuntimeError as e:
        msg = str(e)
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to delete saved SQL")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
