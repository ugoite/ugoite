"""Attachment endpoints."""

import logging
from typing import Annotated, Any

import ieapp_core
from fastapi import APIRouter, File, HTTPException, UploadFile, status

from app.api.endpoints.workspace import (
    _ensure_workspace_exists,
    _storage_config,
    _validate_path_id,
)

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/workspaces/{workspace_id}/attachments",
    status_code=status.HTTP_201_CREATED,
)
async def upload_attachment_endpoint(
    workspace_id: str,
    file: Annotated[UploadFile, File(...)],
) -> dict[str, Any]:
    """Upload a binary attachment into the workspace attachments directory."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    contents = await file.read()

    try:
        return await ieapp_core.save_attachment(
            storage_config,
            workspace_id,
            file.filename or "",
            contents,
        )
    except Exception as e:
        logger.exception("Failed to save attachment")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/attachments")
async def list_attachments_endpoint(
    workspace_id: str,
) -> list[dict[str, Any]]:
    """List all attachments in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_attachments(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list attachments")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/workspaces/{workspace_id}/attachments/{attachment_id}")
async def delete_attachment_endpoint(
    workspace_id: str,
    attachment_id: str,
) -> dict[str, str]:
    """Delete an attachment if it is not referenced by any note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(attachment_id, "attachment_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await ieapp_core.delete_attachment(storage_config, workspace_id, attachment_id)
    except RuntimeError as e:
        msg = str(e)
        if "referenced" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=msg,
            ) from e
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Not found",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to delete attachment")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"status": "deleted", "id": attachment_id}
