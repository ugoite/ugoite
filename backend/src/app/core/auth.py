"""FastAPI adapter for ugoite-core authentication foundation."""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request, status
from ugoite_core.auth import (
    AuthError,
    RequestIdentity,
    authenticate_headers,
    authenticate_headers_for_space,
    clear_auth_manager_cache,
)


def authenticate_request(request: Request) -> RequestIdentity:
    """Resolve authenticated identity from request headers."""
    return authenticate_headers(request.headers)


async def authenticate_request_for_space(
    request: Request,
    storage_config: dict[str, str],
    space_id: str,
) -> RequestIdentity:
    """Resolve identity for a space-scoped request including service account keys."""
    return await authenticate_headers_for_space(
        storage_config,
        space_id,
        request.headers,
        request_method=request.method,
        request_path=request.url.path,
        request_id=request.headers.get("x-request-id"),
    )


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
    "authenticate_request_for_space",
    "clear_auth_manager_cache",
    "require_authenticated_identity",
]
