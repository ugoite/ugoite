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

from app.core.auth import (
    AuthError,
    authenticate_request,
    authenticate_request_for_space,
)
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
    "/health",
    "/auth/config",
    "/auth/login",
    "/auth/mock-oauth",
}
_DEFAULT_SIGNATURE_SPACE_ID = "default"


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


def _request_uses_https(
    request: Request,
    *,
    trust_proxy_headers: bool,
) -> bool:
    """Determine whether the effective request scheme is HTTPS."""
    if trust_proxy_headers:
        forwarded_proto = request.headers.get("x-forwarded-proto", "")
        effective_proto = forwarded_proto.split(",", 1)[0].strip().lower()
        if effective_proto:
            return effective_proto == "https"
    return request.url.scheme.lower() == "https"


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
    signature_space_id = (
        _space_id_from_path(request.url.path) or _DEFAULT_SIGNATURE_SPACE_ID
    )
    # 1. Localhost Binding Check (unless disabled via env var)
    allow_remote = os.environ.get("UGOITE_ALLOW_REMOTE", "false").lower() == "true"
    trust_proxy_headers = (
        os.environ.get("UGOITE_TRUST_PROXY_HEADERS", "false").lower() == "true"
    )
    client_host = resolve_client_host(
        request.headers,
        request.client.host if request.client else None,
        trust_proxy_headers=trust_proxy_headers,
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
        return await _apply_security_headers(
            response,
            body,
            root_path,
            signature_space_id,
            uses_https=_request_uses_https(
                request,
                trust_proxy_headers=trust_proxy_headers,
            ),
        )

    if not _is_auth_exempt(request.url.path):
        try:
            space_id = _space_id_from_path(request.url.path)
            storage_config = storage_config_from_root(root_path)
            if isinstance(space_id, str):
                identity = await authenticate_request_for_space(
                    request,
                    storage_config,
                    space_id,
                )
            else:
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
            return await _apply_security_headers(
                response,
                body,
                root_path,
                signature_space_id,
                uses_https=_request_uses_https(
                    request,
                    trust_proxy_headers=trust_proxy_headers,
                ),
            )

    response = await call_next(request)

    # Skip body capture/signing only for SSE responses that cannot be buffered
    content_type = response.headers.get("content-type", "")
    if content_type.lower().startswith("text/event-stream"):
        return response

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

    uses_https = _request_uses_https(
        request,
        trust_proxy_headers=trust_proxy_headers,
    )
    return await _apply_security_headers(
        response,
        body,
        root_path,
        signature_space_id,
        uses_https=uses_https,
    )


async def _capture_response_body(response: Response) -> bytes:
    """Consume the response iterator so it can be signed and replayed."""
    body_iterator = getattr(response, "body_iterator", None)

    if body_iterator is None:
        return bytes(response.body or b"")

    body = b"".join([bytes(chunk) async for chunk in body_iterator])
    response.body_iterator = iterate_in_threadpool(iter([body]))  # ty: ignore[unresolved-attribute]
    return body


async def _apply_security_headers(
    response: Response,
    body: bytes,
    root_path: Path | str,
    space_id: str,
    *,
    uses_https: bool,
) -> Response:
    """Attach security-related headers including the HMAC signature."""
    if space_id == "default":
        key_id, signature = await build_response_signature(body, root_path)
    else:
        key_id, signature = await build_response_signature(body, root_path, space_id)
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["X-Frame-Options"] = "DENY"
    response.headers["Referrer-Policy"] = "strict-origin-when-cross-origin"
    response.headers["Permissions-Policy"] = (
        "camera=(), microphone=(), geolocation=()"
    )
    response.headers["Content-Security-Policy"] = (
        "default-src 'self'; script-src 'self'; object-src 'none'; "
        "frame-ancestors 'none'"
    )
    if uses_https:
        response.headers["Strict-Transport-Security"] = "max-age=31536000"
    response.headers["X-Ugoite-Key-Id"] = key_id
    response.headers["X-Ugoite-Signature"] = signature
    response.headers["Content-Length"] = str(len(body))
    return response
