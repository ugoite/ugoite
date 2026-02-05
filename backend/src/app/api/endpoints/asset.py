"""Asset endpoints."""

import logging
from typing import Annotated, Any

import ieapp_core
from fastapi import APIRouter, File, HTTPException, UploadFile, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/spaces/{space_id}/assets",
    status_code=status.HTTP_201_CREATED,
)
async def upload_asset_endpoint(
    space_id: str,
    file: Annotated[UploadFile, File(...)],
) -> dict[str, Any]:
    """Upload a binary asset into the space assets directory."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    contents = await file.read()

    try:
        return await ieapp_core.save_asset(
            storage_config,
            space_id,
            file.filename or "",
            contents,
        )
    except Exception as e:
        logger.exception("Failed to save asset")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/assets")
async def list_assets_endpoint(
    space_id: str,
) -> list[dict[str, Any]]:
    """List all assets in the space."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ieapp_core.list_assets(storage_config, space_id)
    except Exception as e:
        logger.exception("Failed to list assets")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/spaces/{space_id}/assets/{asset_id}")
async def delete_asset_endpoint(
    space_id: str,
    asset_id: str,
) -> dict[str, str]:
    """Delete an asset if it is not referenced by any entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(asset_id, "asset_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ieapp_core.delete_asset(storage_config, space_id, asset_id)
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
        logger.exception("Failed to delete asset")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"status": "deleted", "id": asset_id}
