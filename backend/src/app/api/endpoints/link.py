"""Link endpoints."""

import logging
import uuid
from typing import Any

import ieapp_core
from fastapi import APIRouter, HTTPException, status

from app.api.endpoints.workspace import (
    _ensure_workspace_exists,
    _storage_config,
    _validate_path_id,
)
from app.models.classes import LinkCreate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/workspaces/{workspace_id}/links",
    status_code=status.HTTP_201_CREATED,
)
async def create_link_endpoint(
    workspace_id: str,
    payload: LinkCreate,
) -> dict[str, Any]:
    """Create a bi-directional link between two notes."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(payload.source, "source")
    _validate_path_id(payload.target, "target")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    link_id = uuid.uuid4().hex

    try:
        return await ieapp_core.create_link(
            storage_config,
            workspace_id,
            payload.source,
            payload.target,
            payload.kind,
            link_id,
        )
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create link")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/links")
async def list_links_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all unique links in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_links(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list links")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/workspaces/{workspace_id}/links/{link_id}")
async def delete_link_endpoint(
    workspace_id: str,
    link_id: str,
) -> dict[str, str]:
    """Delete a link and remove it from both notes."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(link_id, "link_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await ieapp_core.delete_link(storage_config, workspace_id, link_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Link not found",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to delete link")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"status": "deleted", "id": link_id}
