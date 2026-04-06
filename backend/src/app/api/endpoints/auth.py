"""Explicit passwordless login endpoints."""

import os
import secrets
import time
from dataclasses import dataclass, field
from math import ceil
from threading import Lock
from typing import Literal

import ugoite_core
from fastapi import APIRouter, HTTPException, Request, status
from ugoite_core.auth import mint_signed_bearer_token, validate_totp_code

from app.core.config import get_root_path
from app.core.security import is_local_host, resolve_client_host
from app.core.storage import storage_config_from_root
from app.models.payloads import AuthLogin

router = APIRouter(prefix="/auth", tags=["auth"])

AuthMode = Literal["passkey-totp", "mock-oauth"]
DEFAULT_DEV_AUTH_MODE: AuthMode = "passkey-totp"
DEFAULT_DEV_AUTH_TTL_SECONDS = 43_200
DEV_AUTH_PROXY_HEADER_NAME = "x-ugoite-dev-auth-proxy-token"
DEV_PASSKEY_CONTEXT_HEADER_NAME = "x-ugoite-dev-passkey-context"
DEV_PASSKEY_CONTEXT_ENV_NAME = "UGOITE_DEV_PASSKEY_CONTEXT"
LOGIN_FAILURE_LIMIT = 5
LOGIN_FAILURE_WINDOW_SECONDS = 60.0
LOGIN_LOCKOUT_SECONDS = 60.0
TRUSTED_DEV_AUTH_PROXY_SCOPE = "trusted-dev-auth-proxy"


@dataclass(slots=True)
class _LoginAttemptState:
    failed_at: list[float] = field(default_factory=list)
    locked_until: float | None = None


_LOGIN_ATTEMPTS: dict[str, _LoginAttemptState] = {}
_LOGIN_ATTEMPTS_LOCK = Lock()


def _default_dev_token_kid() -> str:
    return "{}-{}-{}".format("dev", "local", "v1")


def _resolve_dev_auth_mode() -> AuthMode:
    raw_mode = os.environ.get("UGOITE_DEV_AUTH_MODE", DEFAULT_DEV_AUTH_MODE).strip()
    if raw_mode == "passkey-totp":
        return "passkey-totp"
    if raw_mode == "mock-oauth":
        return "mock-oauth"
    message = "Unsupported UGOITE_DEV_AUTH_MODE. Expected passkey-totp or mock-oauth."
    raise HTTPException(
        status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
        detail=message,
    )


def _dev_user_id() -> str:
    user_id = os.environ.get("UGOITE_DEV_USER_ID", "dev-local-user").strip()
    if not user_id:
        message = "UGOITE_DEV_USER_ID must be configured for explicit login."
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=message,
        )
    return user_id


def _dev_signing_material() -> tuple[str, str]:
    key_id = os.environ.get("UGOITE_DEV_SIGNING_KID", _default_dev_token_kid()).strip()
    secret = os.environ.get("UGOITE_DEV_SIGNING_SECRET", "").strip()
    if not key_id or not secret:
        message = (
            "Passwordless login is unavailable because signing material is missing."
        )
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=message,
        )
    return key_id, secret


def _dev_auth_ttl_seconds() -> int:
    raw = os.environ.get(
        "UGOITE_DEV_AUTH_TTL_SECONDS",
        str(DEFAULT_DEV_AUTH_TTL_SECONDS),
    )
    try:
        ttl_seconds = int(raw)
    except ValueError as exc:
        message = "UGOITE_DEV_AUTH_TTL_SECONDS must be an integer."
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=message,
        ) from exc
    if ttl_seconds <= 0:
        message = "UGOITE_DEV_AUTH_TTL_SECONDS must be positive."
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=message,
        )
    return ttl_seconds


def _trusted_dev_auth_proxy_token() -> str | None:
    token = os.environ.get("UGOITE_DEV_AUTH_PROXY_TOKEN")
    if token is None:
        return None
    normalized = token.strip()
    return normalized or None


def _required_dev_passkey_context() -> str:
    context = os.environ.get(DEV_PASSKEY_CONTEXT_ENV_NAME, "").strip()
    if context:
        return context
    raise HTTPException(
        status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
        detail=(
            "Failed to configure passkey-totp login: "
            f"{DEV_PASSKEY_CONTEXT_ENV_NAME} must be configured."
        ),
    )


def _is_trusted_dev_auth_proxy(request: Request) -> bool:
    configured_token = _trusted_dev_auth_proxy_token()
    if configured_token is None:
        return False

    provided_token = request.headers.get(DEV_AUTH_PROXY_HEADER_NAME)
    if provided_token is None or not provided_token:
        return False

    return secrets.compare_digest(provided_token, configured_token)


def _resolve_local_dev_auth_client_scope(request: Request) -> str:
    trust_proxy_headers = (
        os.environ.get("UGOITE_TRUST_PROXY_HEADERS", "false").lower() == "true"
    )
    client_host = resolve_client_host(
        request.headers,
        request.client.host if request.client else None,
        trust_proxy_headers=trust_proxy_headers,
    )
    if client_host is not None and is_local_host(client_host):
        return client_host
    if _is_trusted_dev_auth_proxy(request):
        return TRUSTED_DEV_AUTH_PROXY_SCOPE

    raise HTTPException(
        status_code=status.HTTP_403_FORBIDDEN,
        detail="Explicit login endpoints are only available from loopback clients.",
    )


def _ensure_local_dev_auth_request(request: Request) -> None:
    _resolve_local_dev_auth_client_scope(request)


def _issue_dev_bearer_token(user_id: str) -> dict[str, int | str]:
    key_id, secret = _dev_signing_material()
    expires_at = int(time.time()) + _dev_auth_ttl_seconds()
    return {
        "bearer_token": mint_signed_bearer_token(
            user_id=user_id,
            key_id=key_id,
            secret=secret,
            expires_at=expires_at,
        ),
        "expires_at": expires_at,
        "user_id": user_id,
    }


def _storage_config() -> dict[str, str]:
    """Build storage config for login bootstrap operations."""
    return storage_config_from_root(get_root_path())


def _ensure_dev_passkey_context(request: Request) -> None:
    provided_context = request.headers.get(DEV_PASSKEY_CONTEXT_HEADER_NAME)
    if provided_context is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Passkey-bound local context is missing or invalid.",
        )
    normalized_context = provided_context.strip()
    if not normalized_context:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Passkey-bound local context is missing or invalid.",
        )
    expected_context = _required_dev_passkey_context()
    if not secrets.compare_digest(normalized_context, expected_context):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Passkey-bound local context is missing or invalid.",
        )


def _login_attempt_key(client_scope: str, user_id: str) -> str:
    return f"{client_scope}:{user_id}"


def _prune_login_attempt_state(
    key: str,
    *,
    now: float,
) -> _LoginAttemptState | None:
    state = _LOGIN_ATTEMPTS.get(key)
    if state is None:
        return None

    if state.locked_until is not None and state.locked_until <= now:
        state.locked_until = None
        state.failed_at.clear()

    cutoff = now - LOGIN_FAILURE_WINDOW_SECONDS
    state.failed_at[:] = [
        failed_at for failed_at in state.failed_at if failed_at >= cutoff
    ]

    if state.locked_until is None and not state.failed_at:
        _LOGIN_ATTEMPTS.pop(key, None)
        return None
    return state


def _raise_login_throttled(wait_seconds: int) -> None:
    raise HTTPException(
        status_code=status.HTTP_429_TOO_MANY_REQUESTS,
        detail=f"Too many failed login attempts. Try again in {wait_seconds} seconds.",
        headers={"Retry-After": str(wait_seconds)},
    )


def _enforce_login_throttle(key: str) -> None:
    with _LOGIN_ATTEMPTS_LOCK:
        now = time.monotonic()
        state = _prune_login_attempt_state(key, now=now)
        if state is None or state.locked_until is None:
            return
        wait_seconds = max(1, ceil(state.locked_until - now))

    _raise_login_throttled(wait_seconds)


def _record_login_failure(key: str) -> int | None:
    with _LOGIN_ATTEMPTS_LOCK:
        now = time.monotonic()
        state = _prune_login_attempt_state(key, now=now)
        if state is None:
            state = _LoginAttemptState()
            _LOGIN_ATTEMPTS[key] = state
        state.failed_at.append(now)
        if len(state.failed_at) < LOGIN_FAILURE_LIMIT:
            return None

        state.locked_until = now + LOGIN_LOCKOUT_SECONDS
        return max(1, ceil(state.locked_until - now))


def _clear_login_failures(key: str) -> None:
    with _LOGIN_ATTEMPTS_LOCK:
        _LOGIN_ATTEMPTS.pop(key, None)


def clear_login_attempts() -> None:
    """Reset in-memory login throttling state."""
    with _LOGIN_ATTEMPTS_LOCK:
        _LOGIN_ATTEMPTS.clear()


@router.get("/config")
async def auth_config_endpoint(request: Request) -> dict[str, object]:
    """Expose the current passwordless login mode to browser/CLI clients."""
    _ensure_local_dev_auth_request(request)
    mode = _resolve_dev_auth_mode()
    return {
        "mode": mode,
        "username_hint": _dev_user_id(),
        "supports_passkey_totp": mode == "passkey-totp",
        "supports_mock_oauth": mode == "mock-oauth",
    }


@router.post("/login")
async def login_endpoint(
    payload: AuthLogin,
    request: Request,
) -> dict[str, int | str]:
    """Validate passkey-bound local context + TOTP and issue a signed bearer token."""
    client_scope = _resolve_local_dev_auth_client_scope(request)
    if _resolve_dev_auth_mode() != "passkey-totp":
        message = "passkey-totp login is not enabled for this session."
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=message,
        )
    _ensure_dev_passkey_context(request)

    expected_username = _dev_user_id()
    attempt_key = _login_attempt_key(client_scope, expected_username)
    _enforce_login_throttle(attempt_key)
    if payload.username != expected_username:
        wait_seconds = _record_login_failure(attempt_key)
        if wait_seconds is not None:
            _raise_login_throttled(wait_seconds)
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid username or 2FA code.",
        )

    dev_secret = os.environ.get("UGOITE_DEV_2FA_SECRET", "").strip()
    if not dev_secret:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="UGOITE_DEV_2FA_SECRET must be configured for passkey-totp login.",
        )

    if not validate_totp_code(payload.totp_code, dev_secret):
        wait_seconds = _record_login_failure(attempt_key)
        if wait_seconds is not None:
            _raise_login_throttled(wait_seconds)
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid username or 2FA code.",
        )

    _clear_login_failures(attempt_key)
    await ugoite_core.ensure_admin_space(_storage_config(), expected_username)
    return _issue_dev_bearer_token(expected_username)


@router.post("/mock-oauth")
async def mock_oauth_login_endpoint(request: Request) -> dict[str, int | str]:
    """Issue a signed bearer token for explicit mock OAuth login."""
    _ensure_local_dev_auth_request(request)
    if _resolve_dev_auth_mode() != "mock-oauth":
        message = "mock-oauth login is not enabled for this session."
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=message,
        )

    user_id = _dev_user_id()
    await ugoite_core.ensure_admin_space(_storage_config(), user_id)
    return _issue_dev_bearer_token(user_id)
