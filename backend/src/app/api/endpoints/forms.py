"""Form endpoints."""

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
from app.models.payloads import FormCreate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.get("/spaces/{space_id}/forms")
async def list_forms_endpoint(space_id: str) -> list[dict[str, Any]]:
    """List all forms in the space."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ieapp_core.list_forms(storage_config, space_id)
    except Exception as e:
        logger.exception("Failed to list forms")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/forms/types")
async def list_form_types_endpoint(space_id: str) -> list[str]:
    """Get list of available column types."""
    _validate_path_id(space_id, "space_id")
    try:
        # Verify space exists even though types are static
        storage_config = _storage_config()
        await _ensure_space_exists(storage_config, space_id)
        return await ieapp_core.list_column_types()
    except Exception as e:
        if isinstance(e, HTTPException):
            raise
        logger.exception("Failed to list form types")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/forms/{form_name}")
async def get_form_endpoint(space_id: str, form_name: str) -> dict[str, Any]:
    """Get a specific form definition."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(form_name, "form_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ieapp_core.get_form(storage_config, space_id, form_name)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Form not found: {form_name}",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/spaces/{space_id}/forms", status_code=status.HTTP_201_CREATED)
async def create_form_endpoint(
    space_id: str,
    payload: FormCreate,
) -> dict[str, Any]:
    """Create or update a form definition."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(payload.name, "form_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        # Separate strategies from persistent form definition
        form_data = payload.model_dump()
        strategies = form_data.pop("strategies", None)
        form_json = json.dumps(form_data)

        await ieapp_core.upsert_form(storage_config, space_id, form_json)

        if strategies:
            strategies_json = json.dumps(strategies)
            await ieapp_core.migrate_form(
                storage_config,
                space_id,
                form_json,
                strategies_json,
            )

        return await ieapp_core.get_form(storage_config, space_id, payload.name)
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
        logger.exception("Failed to upsert form")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
