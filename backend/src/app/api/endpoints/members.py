"""Space member and invitation endpoints."""

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
    SpaceMemberAccept,
    SpaceMemberInvite,
    SpaceMemberRoleUpdate,
)

router = APIRouter()


@router.get("/spaces/{space_id}/members")
async def list_members_endpoint(
    space_id: str,
    request: Request,
) -> list[dict[str, Any]]:
    """List members in the target space."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_read",
        )
        return await ugoite_core.list_members(storage_config, space_id)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        if "not found" in str(exc).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(exc),
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(exc),
        ) from exc


@router.post(
    "/spaces/{space_id}/members/invitations",
    status_code=status.HTTP_201_CREATED,
)
async def invite_member_endpoint(
    space_id: str,
    payload: SpaceMemberInvite,
    request: Request,
) -> dict[str, Any]:
    """Create a new invitation for a space member."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(payload.user_id, "user_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.create_invitation(
            storage_config,
            space_id,
            ugoite_core.InviteMemberInput(
                user_id=payload.user_id,
                role=payload.role,
                invited_by_user_id=identity.user_id,
                email=payload.email,
                expires_in_seconds=payload.expires_in_seconds or 7 * 24 * 60 * 60,
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
        if "already active" in lowered:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.post("/spaces/{space_id}/members/accept")
async def accept_member_invitation_endpoint(
    space_id: str,
    payload: SpaceMemberAccept,
    request: Request,
) -> dict[str, Any]:
    """Accept a pending invitation token as the authenticated user."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.accept_invitation(
            storage_config,
            space_id,
            ugoite_core.AcceptInvitationInput(
                token=payload.token,
                accepted_by_user_id=identity.user_id,
            ),
        )
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        if "expired" in lowered:
            raise HTTPException(
                status_code=status.HTTP_410_GONE,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=message,
        ) from exc


@router.post("/spaces/{space_id}/members/{member_user_id}/role")
async def update_member_role_endpoint(
    space_id: str,
    member_user_id: str,
    payload: SpaceMemberRoleUpdate,
    request: Request,
) -> dict[str, Any]:
    """Update role assignment for an existing member."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(member_user_id, "member_user_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.update_member_role(
            storage_config,
            space_id,
            ugoite_core.UpdateMemberRoleInput(
                member_user_id=member_user_id,
                role=payload.role,
                changed_by_user_id=identity.user_id,
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


@router.delete("/spaces/{space_id}/members/{member_user_id}")
async def revoke_member_endpoint(
    space_id: str,
    member_user_id: str,
    request: Request,
) -> dict[str, Any]:
    """Revoke member access in the target space."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(member_user_id, "member_user_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.revoke_member(
            storage_config,
            space_id,
            ugoite_core.RevokeMemberInput(
                member_user_id=member_user_id,
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
