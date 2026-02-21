"""Service-account API key management endpoints."""

from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, Request, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.core.authorization import raise_authorization_http_error, request_identity
from app.models.payloads import (
    ServiceAccountCreate,
    ServiceAccountKeyCreate,
    ServiceAccountKeyRotate,
)

router = APIRouter()


@router.get("/spaces/{space_id}/service-accounts")
async def list_service_accounts_endpoint(
    space_id: str,
    request: Request,
) -> list[dict[str, Any]]:
    """List service accounts for the target space."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)
    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.list_service_accounts(storage_config, space_id)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.post("/spaces/{space_id}/service-accounts", status_code=status.HTTP_201_CREATED)
async def create_service_account_endpoint(
    space_id: str,
    payload: ServiceAccountCreate,
    request: Request,
) -> dict[str, Any]:
    """Create a scoped service account."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)
    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.create_service_account(
            storage_config,
            space_id,
            ugoite_core.CreateServiceAccountInput(
                display_name=payload.display_name,
                scopes=payload.scopes,
                created_by_user_id=identity.user_id,
            ),
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.post(
    "/spaces/{space_id}/service-accounts/{service_account_id}/keys",
    status_code=status.HTTP_201_CREATED,
)
async def create_service_account_key_endpoint(
    space_id: str,
    service_account_id: str,
    payload: ServiceAccountKeyCreate,
    request: Request,
) -> dict[str, Any]:
    """Create a one-time reveal API key for a service account."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(service_account_id, "service_account_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)
    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.create_service_account_key(
            storage_config,
            space_id,
            ugoite_core.CreateServiceAccountKeyInput(
                service_account_id=service_account_id,
                key_name=payload.key_name,
                created_by_user_id=identity.user_id,
            ),
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.post(
    "/spaces/{space_id}/service-accounts/{service_account_id}/keys/{key_id}/rotate",
    status_code=status.HTTP_201_CREATED,
)
async def rotate_service_account_key_endpoint(
    space_id: str,
    service_account_id: str,
    key_id: str,
    payload: ServiceAccountKeyRotate,
    request: Request,
) -> dict[str, Any]:
    """Rotate a service-account API key and return the replacement secret."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(service_account_id, "service_account_id")
    _validate_path_id(key_id, "key_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)
    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.rotate_service_account_key(
            storage_config,
            space_id,
            ugoite_core.RotateServiceAccountKeyInput(
                service_account_id=service_account_id,
                key_id=key_id,
                rotated_by_user_id=identity.user_id,
                key_name=payload.key_name,
            ),
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.delete("/spaces/{space_id}/service-accounts/{service_account_id}/keys/{key_id}")
async def revoke_service_account_key_endpoint(
    space_id: str,
    service_account_id: str,
    key_id: str,
    request: Request,
) -> dict[str, Any]:
    """Revoke a service-account API key."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(service_account_id, "service_account_id")
    _validate_path_id(key_id, "key_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)
    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.revoke_service_account_key(
            storage_config,
            space_id,
            ugoite_core.RevokeServiceAccountKeyInput(
                service_account_id=service_account_id,
                key_id=key_id,
                revoked_by_user_id=identity.user_id,
            ),
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc
