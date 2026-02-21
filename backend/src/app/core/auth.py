"""FastAPI adapter for ugoite-core authentication foundation."""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request, status
from ugoite_core.auth import (
    AuthError,
    RequestIdentity,
    authenticate_headers,
    clear_auth_manager_cache,
)


def authenticate_request(request: Request) -> RequestIdentity:
    """Resolve authenticated identity from request headers."""
    return authenticate_headers(request.headers)


def require_authenticated_identity(request: Request) -> RequestIdentity:
    """FastAPI dependency returning the authenticated request identity."""
    identity = getattr(request.state, "identity", None)
    if isinstance(identity, RequestIdentity):
        return identity
    raise HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED,
        detail="Authentication required",
    )


AuthenticatedIdentity = Depends(require_authenticated_identity)

__all__ = [
    "AuthError",
    "AuthenticatedIdentity",
    "RequestIdentity",
    "authenticate_request",
    "clear_auth_manager_cache",
    "require_authenticated_identity",
]
