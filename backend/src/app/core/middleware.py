"""Middleware for the application."""

import logging
import os
from collections.abc import Awaitable, Callable
from pathlib import Path

from fastapi import Request, Response, status
from fastapi.responses import JSONResponse
from starlette.concurrency import iterate_in_threadpool

from app.core.config import get_root_path
from app.core.security import (
    build_response_signature,
    is_local_host,
    resolve_client_host,
)

logger = logging.getLogger(__name__)


async def security_middleware(
    request: Request,
    call_next: Callable[[Request], Awaitable[Response]],
) -> Response:
    """Enforce security policies."""
    root_path = get_root_path()
    # 1. Localhost Binding Check (unless disabled via env var)
    allow_remote = os.environ.get("IEAPP_ALLOW_REMOTE", "false").lower() == "true"
    client_host = resolve_client_host(
        request.headers,
        request.client.host if request.client else None,
    )

    if not allow_remote and not is_local_host(client_host):
        logger.warning("Blocking remote request from %s", client_host)
        response = JSONResponse(
            status_code=status.HTTP_403_FORBIDDEN,
            content={
                "detail": (
                    "Remote access is disabled. Set IEAPP_ALLOW_REMOTE=true only on"
                    " trusted networks."
                ),
            },
        )
        body = bytes(response.body or b"")
        return _apply_security_headers(response, body, root_path)

    # Skip body capture/signing for MCP SSE endpoints as they are streaming
    if request.url.path.startswith("/mcp"):
        return await call_next(request)

    response = await call_next(request)
    body = await _capture_response_body(response)
    return _apply_security_headers(response, body, root_path)


async def _capture_response_body(response: Response) -> bytes:
    """Consume the response iterator so it can be signed and replayed."""
    body = b""
    body_iterator = getattr(response, "body_iterator", None)

    if body_iterator is None:
        return bytes(response.body or b"")

    async for chunk in body_iterator:
        body += chunk

    response.body_iterator = iterate_in_threadpool(iter([body]))  # type: ignore[attr-defined]
    return body


def _apply_security_headers(
    response: Response,
    body: bytes,
    root_path: Path | str,
) -> Response:
    """Attach security-related headers including the HMAC signature."""
    key_id, signature = build_response_signature(body, root_path)
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["X-IEApp-Key-Id"] = key_id
    response.headers["X-IEApp-Signature"] = signature
    response.headers["Content-Length"] = str(len(body))
    return response
