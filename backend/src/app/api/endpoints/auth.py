"""Local development authentication endpoints."""

from __future__ import annotations

import os
import time
from typing import Literal

from fastapi import APIRouter, HTTPException, Request, status
from ugoite_core.auth import mint_signed_bearer_token, validate_totp_code

from app.core.security import is_local_host, resolve_client_host
from app.models.payloads import DevAuthLogin

router = APIRouter(prefix="/auth/dev", tags=["auth"])
DEV_AUTH_LOGIN_MODEL = DevAuthLogin

AuthMode = Literal["manual-totp", "mock-oauth"]
DEFAULT_DEV_AUTH_MODE: AuthMode = "manual-totp"
DEFAULT_DEV_AUTH_TTL_SECONDS = 43_200


def _default_dev_token_kid() -> str:
    return "{}-{}-{}".format("dev", "local", "v1")


def _resolve_dev_auth_mode() -> AuthMode:
    raw_mode = os.environ.get("UGOITE_DEV_AUTH_MODE", DEFAULT_DEV_AUTH_MODE).strip()
    if raw_mode in {"manual-totp", "mock-oauth"}:
        return raw_mode
    message = "Unsupported UGOITE_DEV_AUTH_MODE. Expected manual-totp or mock-oauth."
    raise HTTPException(
        status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
        detail=message,
    )


def _dev_user_id() -> str:
    user_id = os.environ.get("UGOITE_DEV_USER_ID", "dev-local-user").strip()
    if not user_id:
        message = "UGOITE_DEV_USER_ID must be configured for local development login."
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
            "Local development login is unavailable because signing material "
            "is missing."
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


def _ensure_local_dev_auth_request(request: Request) -> None:
    trust_proxy_headers = (
        os.environ.get("UGOITE_TRUST_PROXY_HEADERS", "false").lower() == "true"
    )
    client_host = resolve_client_host(
        request.headers,
        request.client.host if request.client else None,
        trust_proxy_headers=trust_proxy_headers,
    )
    if is_local_host(client_host):
        return

    raise HTTPException(
        status_code=status.HTTP_403_FORBIDDEN,
        detail=(
            "Local development auth endpoints are only available from loopback clients."
        ),
    )


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


@router.get("/config")
async def dev_auth_config_endpoint(request: Request) -> dict[str, object]:
    """Expose the current local development login mode to browser/CLI clients."""
    _ensure_local_dev_auth_request(request)
    mode = _resolve_dev_auth_mode()
    return {
        "mode": mode,
        "username_hint": _dev_user_id(),
        "supports_manual_totp": mode == "manual-totp",
        "supports_mock_oauth": mode == "mock-oauth",
    }


@router.post("/login")
async def dev_login_endpoint(
    payload: DevAuthLogin,
    request: Request,
) -> dict[str, int | str]:
    """Validate local development username + TOTP and issue a signed bearer token."""
    _ensure_local_dev_auth_request(request)
    DEV_AUTH_LOGIN_MODEL.model_validate(payload.model_dump())
    if _resolve_dev_auth_mode() != "manual-totp":
        message = "manual-totp login is not enabled for this local development session."
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=message,
        )

    expected_username = _dev_user_id()
    if payload.username != expected_username:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid username or 2FA code.",
        )

    dev_secret = os.environ.get("UGOITE_DEV_2FA_SECRET", "").strip()
    if not dev_secret:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="UGOITE_DEV_2FA_SECRET must be configured for manual-totp login.",
        )

    if not validate_totp_code(payload.totp_code, dev_secret):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid username or 2FA code.",
        )

    return _issue_dev_bearer_token(expected_username)


@router.post("/mock-oauth")
async def dev_mock_oauth_login_endpoint(request: Request) -> dict[str, int | str]:
    """Issue a signed bearer token for explicit mock OAuth login."""
    _ensure_local_dev_auth_request(request)
    if _resolve_dev_auth_mode() != "mock-oauth":
        message = "mock-oauth login is not enabled for this local development session."
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=message,
        )

    return _issue_dev_bearer_token(_dev_user_id())
