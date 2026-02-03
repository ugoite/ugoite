"""Search and query endpoints."""

import json
import logging
from typing import Annotated, Any

import ieapp_core
from fastapi import APIRouter, HTTPException, Query, status

from app.api.endpoints.workspace import (
    _ensure_workspace_exists,
    _storage_config,
    _validate_path_id,
)
from app.models.classes import QueryRequest

SQL_ERROR_PREFIX = "IEAPP_SQL_ERROR"


def _is_sql_error(detail: str) -> bool:
    return detail.strip().startswith(f"{SQL_ERROR_PREFIX}:")


router = APIRouter()
logger = logging.getLogger(__name__)


@router.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    if payload.filter.get("$sql") or payload.filter.get("sql"):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="SQL queries must use SQL session endpoints.",
        )

    try:
        query_payload = json.dumps(payload.filter)
        return await ieapp_core.query_index(storage_config, workspace_id, query_payload)
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


@router.get("/workspaces/{workspace_id}/search")
async def search_endpoint(
    workspace_id: str,
    q: Annotated[str, Query(..., min_length=1)],
) -> list[dict[str, Any]]:
    """Hybrid keyword search using inverted index with on-demand indexing."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.search_notes(storage_config, workspace_id, q)
    except Exception as e:
        logger.exception("Search failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
