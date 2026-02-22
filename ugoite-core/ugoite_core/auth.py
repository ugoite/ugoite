"""Authentication primitives shared across backend and CLI integrations."""

from __future__ import annotations

import hashlib
import json
import logging
import os
import secrets
from dataclasses import dataclass
from functools import lru_cache
from typing import Literal, NoReturn, cast

from . import _ugoite_core as _core
from .service_accounts import resolve_service_api_key

DEFAULT_UNAUTHORIZED_STATUS_CODE = 401
AUTH_HEADER_PARTS = 2

logger = logging.getLogger(__name__)


def _raise_auth(code: str, detail: str) -> NoReturn:
    raise AuthError(code, detail)


def _header_value(headers: dict[str, str] | object, name: str) -> str | None:
    if isinstance(headers, dict):
        target = name.lower()
        for key, value in headers.items():
            if (
                isinstance(key, str)
                and key.lower() == target
                and isinstance(value, str)
            ):
                return value

    getter = getattr(headers, "get", None)
    if getter is None:
        return None
    for candidate in (name, name.lower(), name.upper()):
        value = getter(candidate)
        if isinstance(value, str):
            return value
    return None


def _as_object_dict(value: object) -> dict[str, object] | None:
    if not isinstance(value, dict):
        return None
    return {key: item for key, item in value.items() if isinstance(key, str)}


@dataclass(frozen=True)
class RequestIdentity:
    """Resolved request identity."""

    user_id: str
    auth_method: Literal["bearer", "api_key"]
    principal_type: Literal["user", "service"] = "user"
    display_name: str | None = None
    key_id: str | None = None
    scopes: frozenset[str] = frozenset()
    scope_enforced: bool = False
    service_account_id: str | None = None


class AuthError(Exception):
    """Authentication error carrying status information."""

    def __init__(
        self,
        code: str,
        detail: str,
        status_code: int = DEFAULT_UNAUTHORIZED_STATUS_CODE,
    ) -> None:
        """Create an auth error with machine-readable `code` and HTTP status."""
        super().__init__(detail)
        self.code = code
        self.detail = detail
        self.status_code = status_code


@dataclass(frozen=True)
class AuthManager:
    """Thin manager delegating auth checks to rust core implementation."""

    bootstrap_token: str | None
    bootstrap_user_id: str

    def authenticate_headers(self, headers: dict[str, str] | object) -> RequestIdentity:
        """Resolve identity from request headers using rust-core auth logic."""
        authorization = _header_value(headers, "authorization")
        api_key = _header_value(headers, "x-api-key")

        if authorization:
            parts = authorization.split(" ", 1)
            if len(parts) != AUTH_HEADER_PARTS or parts[0].lower() != "bearer":
                _raise_auth(
                    "invalid_credentials",
                    "Authorization header must use Bearer scheme",
                )

        raw = _core.authenticate_headers_core(
            authorization=authorization,
            api_key=api_key,
            bearer_tokens_json=os.environ.get("UGOITE_AUTH_BEARER_TOKENS_JSON"),
            api_keys_json=os.environ.get("UGOITE_AUTH_API_KEYS_JSON"),
            bearer_secrets=os.environ.get("UGOITE_AUTH_BEARER_SECRETS"),
            active_kids=os.environ.get("UGOITE_AUTH_BEARER_ACTIVE_KIDS"),
            revoked_key_ids=os.environ.get("UGOITE_AUTH_REVOKED_KEY_IDS"),
            bootstrap_token=self.bootstrap_token,
            bootstrap_user_id=self.bootstrap_user_id,
        )
        payload = _as_object_dict(raw)
        if payload is None or not isinstance(payload.get("ok"), bool):
            _raise_auth("invalid_credentials", "Invalid authentication response")

        if not payload["ok"]:
            error = _as_object_dict(payload.get("error"))
            if error is not None:
                code = error.get("code")
                detail = error.get("detail")
                status_code = error.get("status_code")
                raise AuthError(
                    code if isinstance(code, str) else "invalid_credentials",
                    detail if isinstance(detail, str) else "Authentication failed",
                    int(status_code)
                    if isinstance(status_code, int)
                    else DEFAULT_UNAUTHORIZED_STATUS_CODE,
                )
            _raise_auth("invalid_credentials", "Authentication failed")

        identity = _as_object_dict(payload.get("identity"))
        if identity is None:
            _raise_auth("invalid_credentials", "Missing identity payload")

        user_id = identity.get("user_id")
        auth_method_obj = identity.get("auth_method")
        principal_type_obj = identity.get("principal_type", "user")

        if not isinstance(user_id, str) or not user_id:
            _raise_auth("invalid_credentials", "Invalid identity user_id")
        if auth_method_obj not in {"bearer", "api_key"}:
            _raise_auth("invalid_credentials", "Invalid identity auth_method")
        if principal_type_obj not in {"user", "service"}:
            _raise_auth("invalid_credentials", "Invalid identity principal_type")

        scopes_obj = identity.get("scopes")
        scopes = (
            frozenset(scope for scope in scopes_obj if isinstance(scope, str) and scope)
            if isinstance(scopes_obj, list)
            else frozenset()
        )

        display_name = identity.get("display_name")
        key_id = identity.get("key_id")
        service_account_id = identity.get("service_account_id")

        return RequestIdentity(
            user_id=user_id,
            principal_type=cast("Literal['user', 'service']", principal_type_obj),
            display_name=display_name if isinstance(display_name, str) else None,
            auth_method=cast("Literal['bearer', 'api_key']", auth_method_obj),
            key_id=key_id if isinstance(key_id, str) else None,
            scopes=scopes,
            scope_enforced=bool(identity.get("scope_enforced", False)),
            service_account_id=(
                service_account_id if isinstance(service_account_id, str) else None
            ),
        )


def _token_fingerprint(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()[:12]


def _has_valid_bearer_tokens_config() -> bool:
    raw = os.environ.get("UGOITE_AUTH_BEARER_TOKENS_JSON")
    if raw is None or not raw.strip():
        return False
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        logger.warning(
            "UGOITE_AUTH_BEARER_TOKENS_JSON is not valid JSON; "
            "falling back to bootstrap token configuration.",
        )
        return False
    return isinstance(parsed, dict)


@lru_cache(maxsize=1)
def get_auth_manager() -> AuthManager:
    """Create and cache authentication manager from environment settings."""
    bootstrap_token: str | None = None
    if not _has_valid_bearer_tokens_config():
        bootstrap_token = os.environ.get("UGOITE_BOOTSTRAP_BEARER_TOKEN")
        if bootstrap_token is None:
            bootstrap_token = secrets.token_urlsafe(32)
            logger.warning(
                "No bearer credentials configured. "
                "Generated one-time bootstrap token fingerprint=%s; "
                "set UGOITE_BOOTSTRAP_BEARER_TOKEN or UGOITE_AUTH_BEARER_TOKENS_JSON "
                "for deterministic startup credentials.",
                _token_fingerprint(bootstrap_token),
            )

    return AuthManager(
        bootstrap_token=bootstrap_token,
        bootstrap_user_id=os.environ.get("UGOITE_BOOTSTRAP_USER_ID", "bootstrap-user"),
    )


def clear_auth_manager_cache() -> None:
    """Clear cached auth manager for tests and dynamic config updates."""
    get_auth_manager.cache_clear()


def authenticate_headers(headers: dict[str, str] | object) -> RequestIdentity:
    """Resolve authenticated identity from request headers."""
    return get_auth_manager().authenticate_headers(headers)


async def authenticate_headers_for_space(
    storage_config: dict[str, str],
    space_id: str,
    headers: dict[str, str] | object,
    *,
    request_method: str | None = None,
    request_path: str | None = None,
    request_id: str | None = None,
) -> RequestIdentity:
    """Resolve identity with support for space-scoped service-account API keys."""
    authorization = _header_value(headers, "authorization")
    if authorization:
        return authenticate_headers(headers)

    api_key = _header_value(headers, "x-api-key")
    if not api_key:
        return authenticate_headers(headers)

    try:
        return get_auth_manager().authenticate_headers(headers)
    except AuthError as exc:
        if exc.code != "invalid_credentials":
            raise

    try:
        resolved = await resolve_service_api_key(
            storage_config,
            space_id,
            api_key,
            request_method=request_method,
            request_path=request_path,
            request_id=request_id,
        )
    except RuntimeError as exc:
        message = str(exc).lower()
        if "missing" in message:
            _raise_auth("missing_credentials", "Missing API key")
        if "revoked" in message:
            _raise_auth("revoked_key", "API key has been revoked")
        _raise_auth("invalid_credentials", "Invalid API key")

    return RequestIdentity(
        user_id=resolved.user_id,
        principal_type="service",
        display_name=resolved.display_name,
        auth_method="api_key",
        key_id=resolved.key_id,
        scopes=resolved.scopes,
        scope_enforced=True,
        service_account_id=resolved.service_account_id,
    )


def export_authentication_overview() -> dict[str, object]:
    """Return normalized auth capability overview generated from rust core."""
    snapshot = _core.auth_capabilities_snapshot_core(
        bearer_tokens_json=os.environ.get("UGOITE_AUTH_BEARER_TOKENS_JSON"),
        api_keys_json=os.environ.get("UGOITE_AUTH_API_KEYS_JSON"),
        bearer_secrets=os.environ.get("UGOITE_AUTH_BEARER_SECRETS"),
        active_kids=os.environ.get("UGOITE_AUTH_BEARER_ACTIVE_KIDS"),
        revoked_key_ids=os.environ.get("UGOITE_AUTH_REVOKED_KEY_IDS"),
    )
    payload = _as_object_dict(snapshot)
    if payload is None:
        return {}
    payload["channels"] = [
        "backend(rest)",
        "backend(mcp)",
        "cli(via backend)",
        "frontend(via backend)",
    ]
    return payload


def auth_headers_from_environment() -> dict[str, str]:
    """Build outbound auth headers for CLI/frontend calls to backend APIs."""
    bearer_token = os.environ.get("UGOITE_AUTH_BEARER_TOKEN")
    if bearer_token:
        return {"Authorization": f"Bearer {bearer_token}"}

    api_key = os.environ.get("UGOITE_AUTH_API_KEY")
    if api_key:
        return {"X-API-Key": api_key}

    return {}
