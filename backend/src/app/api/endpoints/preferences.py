"""Portable user preference endpoints."""

import json
import logging
from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, Request, status

from app.core.authorization import request_identity
from app.core.config import get_root_path
from app.core.ids import validate_id
from app.core.storage import storage_config_from_root
from app.models.payloads import UserPreferences, UserPreferencesPatch

router = APIRouter()
logger = logging.getLogger(__name__)


def _storage_config() -> dict[str, str]:
    """Build storage config for portable user preferences."""
    return storage_config_from_root(get_root_path())


def _validate_selected_space_id(selected_space_id: str | None) -> None:
    """Validate selected_space_id when present."""
    if selected_space_id is None:
        return
    try:
        validate_id(selected_space_id, "selected_space_id")
    except ValueError as exc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(exc),
        ) from exc


@router.get("/preferences/me", response_model=UserPreferences)
async def get_my_preferences_endpoint(request: Request) -> dict[str, Any]:
    """Return portable preferences for the authenticated user."""
    identity = request_identity(request)
    storage_config = _storage_config()
    try:
        return await ugoite_core.get_user_preferences(storage_config, identity.user_id)
    except RuntimeError as exc:
        logger.warning("Failed to load preferences for %s: %s", identity.user_id, exc)
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="Failed to load preferences",
        ) from exc


@router.patch("/preferences/me", response_model=UserPreferences)
async def patch_my_preferences_endpoint(
    request: Request,
    payload: UserPreferencesPatch,
) -> dict[str, Any]:
    """Patch portable preferences for the authenticated user."""
    identity = request_identity(request)
    patch_payload = payload.model_dump(exclude_unset=True)
    if "selected_space_id" in patch_payload:
        _validate_selected_space_id(payload.selected_space_id)

    storage_config = _storage_config()
    try:
        return await ugoite_core.patch_user_preferences(
            storage_config,
            identity.user_id,
            json.dumps(patch_payload),
        )
    except RuntimeError as exc:
        logger.warning("Failed to update preferences for %s: %s", identity.user_id, exc)
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="Failed to update preferences",
        ) from exc
