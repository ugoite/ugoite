"""Asset endpoints."""

import logging
from typing import Annotated, Any

import ugoite_core
from fastapi import APIRouter, File, HTTPException, Request, UploadFile, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.core.authorization import (
    raise_authorization_http_error,
    request_identity,
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
    request: Request,
) -> dict[str, Any]:
    """Upload a binary asset into the space assets directory."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    contents = await file.read()

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "asset_write",
        )
        return await ugoite_core.save_asset(
            storage_config,
            space_id,
            file.filename or "",
            contents,
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to save asset")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/assets")
async def list_assets_endpoint(
    space_id: str,
    request: Request,
) -> list[dict[str, Any]]:
    """List all assets in the space."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "asset_read",
        )
        return await ugoite_core.list_assets(storage_config, space_id)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
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
    request: Request,
) -> dict[str, str]:
    """Delete an asset if it is not referenced by any entry."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(asset_id, "asset_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "asset_write",
        )
        await ugoite_core.delete_asset(storage_config, space_id, asset_id)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
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
