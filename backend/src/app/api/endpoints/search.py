"""Search and query endpoints."""

import json
import logging
from typing import Annotated, Any

import ugoite_core
from fastapi import APIRouter, HTTPException, Query, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.models.payloads import QueryRequest

SQL_ERROR_PREFIX = "UGOITE_SQL_ERROR"


def _is_sql_error(detail: str) -> bool:
    return detail.strip().startswith(f"{SQL_ERROR_PREFIX}:")


router = APIRouter()
logger = logging.getLogger(__name__)


@router.post("/spaces/{space_id}/query")
async def query_endpoint(
    space_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the space index."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    if payload.filter.get("$sql") or payload.filter.get("sql"):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="SQL queries must use SQL session endpoints.",
        )

    try:
        query_payload = json.dumps(payload.filter)
        return await ugoite_core.query_index(storage_config, space_id, query_payload)
    except Exception as e:
        logger.exception("Query failed")
        detail = str(e)
        status_code = (
            status.HTTP_400_BAD_REQUEST
            if _is_sql_error(detail)
            else status.HTTP_500_INTERNAL_SERVER_ERROR
        )
        raise HTTPException(
            status_code=status_code,
            detail=detail,
        ) from e


@router.get("/spaces/{space_id}/search")
async def search_endpoint(
    space_id: str,
    q: Annotated[str, Query(..., min_length=1)],
) -> list[dict[str, Any]]:
    """Hybrid keyword search using inverted index with on-demand indexing."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.search_entries(storage_config, space_id, q)
    except Exception as e:
        logger.exception("Search failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
