"""Audit log retrieval endpoints."""

from __future__ import annotations

from typing import Annotated, Any

import ugoite_core
from fastapi import APIRouter, Depends, HTTPException, Request, status
from pydantic import BaseModel

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.core.authorization import raise_authorization_http_error, request_identity

router = APIRouter()


class AuditQueryParams(BaseModel):
    """Query params for audit event listing."""

    offset: int = 0
    limit: int = 100
    action: str | None = None
    actor_user_id: str | None = None
    outcome: str | None = None


@router.get("/spaces/{space_id}/audit/events")
async def list_audit_events_endpoint(
    space_id: str,
    request: Request,
    params: Annotated[AuditQueryParams, Depends()],
) -> dict[str, Any]:
    """List tamper-evident audit events with pagination and filters."""
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
        return await ugoite_core.list_audit_events(
            storage_config,
            space_id,
            filters=ugoite_core.AuditListFilter(
                offset=max(0, params.offset),
                limit=max(1, min(params.limit, 500)),
                action=params.action,
                actor_user_id=params.actor_user_id,
                outcome=params.outcome,
            ),
        )
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as exc:
        message = str(exc)
        lowered = message.lower()
        if "integrity" in lowered or "chain" in lowered:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=message,
            ) from exc
        if "not found" in lowered:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=message,
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=message,
        ) from exc
