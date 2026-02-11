"""Class endpoints."""

import json
import logging
from typing import Any

import ieapp_core
from fastapi import APIRouter, HTTPException, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.models.classes import ClassCreate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.get("/workspaces/{workspace_id}/classes")
async def list_classes_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all classes in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_classes(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list classes")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/classes/types")
async def list_class_types_endpoint(workspace_id: str) -> list[str]:
    """Get list of available column types."""
    _validate_path_id(workspace_id, "workspace_id")
    try:
        # Verify workspace exists even though types are static
        storage_config = _storage_config()
        await _ensure_space_exists(storage_config, workspace_id)
        return await ieapp_core.list_column_types()
    except Exception as e:
        if isinstance(e, HTTPException):
            raise
        logger.exception("Failed to list class types")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/classes/{class_name}")
async def get_class_endpoint(workspace_id: str, class_name: str) -> dict[str, Any]:
    """Get a specific class definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(class_name, "class_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.get_class(storage_config, workspace_id, class_name)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Class not found: {class_name}",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces/{workspace_id}/classes", status_code=status.HTTP_201_CREATED)
async def create_class_endpoint(
    workspace_id: str,
    payload: ClassCreate,
) -> dict[str, Any]:
    """Create or update a class definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(payload.name, "class_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, workspace_id)

    try:
        # Separate strategies from persistent class definition
        class_data = payload.model_dump()
        strategies = class_data.pop("strategies", None)
        class_json = json.dumps(class_data)

        await ieapp_core.upsert_class(storage_config, workspace_id, class_json)

        if strategies:
            strategies_json = json.dumps(strategies)
            await ieapp_core.migrate_class(
                storage_config,
                workspace_id,
                class_json,
                strategies_json,
            )

        return await ieapp_core.get_class(storage_config, workspace_id, payload.name)
    except RuntimeError as e:
        msg = str(e)
        if "reserved" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to upsert class")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
