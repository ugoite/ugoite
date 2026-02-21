"""Middleware for the application."""

import json
import logging
import os
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from pathlib import Path

import ugoite_core
from fastapi import Request, Response, status
from fastapi.responses import JSONResponse
from starlette.concurrency import iterate_in_threadpool

from app.core.auth import AuthError, authenticate_request
from app.core.config import get_root_path
from app.core.security import (
    build_response_signature,
    is_local_host,
    resolve_client_host,
)
from app.core.storage import storage_config_from_root

logger = logging.getLogger(__name__)

_SUCCESS_STATUS_MIN = status.HTTP_200_OK
_SUCCESS_STATUS_MAX = status.HTTP_400_BAD_REQUEST

_AUTH_EXEMPT_PATHS = {
    "/",
    "/docs",
    "/openapi.json",
    "/redoc",
}


def _is_auth_exempt(path: str) -> bool:
    if path in _AUTH_EXEMPT_PATHS:
        return True
    return path.startswith(("/docs/", "/redoc/"))


def _space_id_from_path(path: str) -> str | None:
    marker = "/spaces/"
    if marker not in path:
        return None
    fragment = path.split(marker, 1)[1]
    space_id = fragment.split("/", 1)[0].strip()
    return space_id or None


@dataclass(frozen=True)
class _AuditRequestEvent:
    action: str
    outcome: str
    actor_user_id: str
    target_type: str | None = None
    target_id: str | None = None
    metadata: dict[str, str] | None = None


async def _emit_audit_event(
    request: Request,
    event: _AuditRequestEvent,
) -> None:
    space_id = _space_id_from_path(request.url.path)
    if space_id is None:
        return

    root_path = get_root_path()
    storage_config = storage_config_from_root(root_path)
    request_id = request.headers.get("x-request-id")
    try:
        await ugoite_core.append_audit_event(
            storage_config,
            space_id,
            ugoite_core.AuditEventInput(
                action=event.action,
                actor_user_id=event.actor_user_id,
                outcome=event.outcome,
                target_type=event.target_type,
                target_id=event.target_id,
                request_method=request.method,
                request_path=request.url.path,
                request_id=request_id,
                metadata=event.metadata,
            ),
        )
    except RuntimeError as exc:
        logger.warning("Failed to write audit event: %s", exc)


async def security_middleware(
    request: Request,
    call_next: Callable[[Request], Awaitable[Response]],
) -> Response:
    """Enforce security policies."""
    root_path = get_root_path()
    # 1. Localhost Binding Check (unless disabled via env var)
    allow_remote = os.environ.get("UGOITE_ALLOW_REMOTE", "false").lower() == "true"
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
                    "Remote access is disabled. Set UGOITE_ALLOW_REMOTE=true only on"
                    " trusted networks."
                ),
            },
        )
        body = bytes(response.body or b"")
        return await _apply_security_headers(response, body, root_path)

    if not _is_auth_exempt(request.url.path):
        try:
            identity = authenticate_request(request)
            request.state.identity = identity
            await _emit_audit_event(
                request,
                _AuditRequestEvent(
                    action="auth.authenticate",
                    outcome="success",
                    actor_user_id=identity.user_id,
                    target_type="space",
                    target_id=_space_id_from_path(request.url.path),
                ),
            )
        except AuthError as exc:
            await _emit_audit_event(
                request,
                _AuditRequestEvent(
                    action="auth.authenticate",
                    outcome="deny",
                    actor_user_id="anonymous",
                    metadata={"code": exc.code},
                ),
            )
            response = JSONResponse(
                status_code=exc.status_code,
                content={
                    "detail": exc.detail,
                    "code": exc.code,
                },
                headers={"WWW-Authenticate": "Bearer"},
            )
            body = bytes(response.body or b"")
            return await _apply_security_headers(response, body, root_path)

    # Skip body capture/signing for MCP SSE endpoints as they are streaming
    if request.url.path.startswith("/mcp"):
        return await call_next(request)

    response = await call_next(request)
    body = await _capture_response_body(response)

    if response.status_code == status.HTTP_403_FORBIDDEN:
        try:
            parsed = json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            parsed = None
        detail = parsed.get("detail") if isinstance(parsed, dict) else None
        if isinstance(detail, dict) and detail.get("code") == "forbidden":
            detail_action = detail.get("action")
            action = detail_action if isinstance(detail_action, str) else "authz.deny"
            identity = getattr(request.state, "identity", None)
            actor = identity.user_id if hasattr(identity, "user_id") else "anonymous"
            await _emit_audit_event(
                request,
                _AuditRequestEvent(
                    action=str(action),
                    outcome="deny",
                    actor_user_id=actor,
                    target_type="space",
                    target_id=_space_id_from_path(request.url.path),
                ),
            )

    if (
        request.method in {"POST", "PUT", "PATCH", "DELETE"}
        and _SUCCESS_STATUS_MIN <= response.status_code < _SUCCESS_STATUS_MAX
    ):
        identity = getattr(request.state, "identity", None)
        actor = identity.user_id if hasattr(identity, "user_id") else "anonymous"
        await _emit_audit_event(
            request,
            _AuditRequestEvent(
                action="data.mutation",
                outcome="success",
                actor_user_id=actor,
                target_type="http_path",
                target_id=request.url.path,
                metadata={"status_code": str(response.status_code)},
            ),
        )

    return await _apply_security_headers(response, body, root_path)


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


async def _apply_security_headers(
    response: Response,
    body: bytes,
    root_path: Path | str,
) -> Response:
    """Attach security-related headers including the HMAC signature."""
    key_id, signature = await build_response_signature(body, root_path)
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["X-Ugoite-Key-Id"] = key_id
    response.headers["X-Ugoite-Signature"] = signature
    response.headers["Content-Length"] = str(len(body))
    return response
