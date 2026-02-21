"""Backend adapter for authorization checks in ugoite-core."""

from __future__ import annotations

from typing import NoReturn

from fastapi import HTTPException, Request, status
from ugoite_core import AuthorizationError, RequestIdentity


def request_identity(request: Request) -> RequestIdentity:
    """Return authenticated identity from request state."""
    identity = getattr(request.state, "identity", None)
    if isinstance(identity, RequestIdentity):
        return identity
    raise HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED,
        detail="Authentication required",
    )


def raise_authorization_http_error(
    exc: AuthorizationError,
    *,
    space_id: str,
) -> NoReturn:
    """Raise consistent authorization error schema for API clients."""
    raise HTTPException(
        status_code=exc.status_code,
        detail={
            "code": exc.code,
            "message": exc.detail,
            "action": exc.action,
            "space_id": space_id,
        },
    )


__all__ = ["raise_authorization_http_error", "request_identity"]
