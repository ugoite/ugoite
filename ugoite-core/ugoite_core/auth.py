"""Authentication primitives shared across backend and CLI integrations."""

from __future__ import annotations

import base64
import binascii
import hashlib
import hmac
import json
import logging
import os
import secrets
import threading
import time
from dataclasses import dataclass
from typing import Literal, NoReturn, cast

from . import _ugoite_core as _core
from .service_accounts import resolve_service_api_key

DEFAULT_UNAUTHORIZED_STATUS_CODE = 401
AUTH_HEADER_PARTS = 2
AUTH_MANAGER_TTL_SECONDS = 60.0
_AUTH_MANAGER_CACHE_LOCK = threading.Lock()

logger = logging.getLogger(__name__)
TOTP_STEP_SECONDS = 30
TOTP_DIGITS = 6
DEFAULT_SIGNED_BEARER_VERSION = "v1"
_LAST_ACCEPTED_TOTP_COUNTERS: dict[str, int] = {}


@dataclass
class _AuthManagerCacheState:
    entry: tuple[float, AuthManager] | None = None
    generated_bootstrap_token: str | None = None


_AUTH_MANAGER_CACHE = _AuthManagerCacheState()


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


def _has_configured_bearer_credentials() -> bool:
    raw = os.environ.get("UGOITE_AUTH_BEARER_TOKENS_JSON")
    if raw is not None and raw.strip():
        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError:
            logger.warning(
                "UGOITE_AUTH_BEARER_TOKENS_JSON is not valid JSON; "
                "falling back to bootstrap token configuration.",
            )
        else:
            if isinstance(parsed, dict):
                return True

    bearer_secrets = os.environ.get("UGOITE_AUTH_BEARER_SECRETS")
    return isinstance(bearer_secrets, str) and bool(bearer_secrets.strip())


def _base64url_encode(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).decode("utf-8").rstrip("=")


def _totp_counter(timestamp: int, *, step_seconds: int) -> int:
    return timestamp // step_seconds


def _hotp_value(secret: bytes, counter: int, *, digits: int) -> str:
    digest = hmac.new(
        secret,
        counter.to_bytes(8, "big"),
        hashlib.sha1,
    ).digest()
    offset = digest[-1] & 0x0F
    binary = int.from_bytes(digest[offset : offset + 4], "big") & 0x7FFFFFFF
    return f"{binary % (10**digits):0{digits}d}"


def _totp_secret_fingerprint(secret: bytes) -> str:
    return hashlib.sha256(secret).hexdigest()


def validate_totp_code(
    code: str,
    secret: str,
    *,
    now: int | None = None,
    step_seconds: int = TOTP_STEP_SECONDS,
    digits: int = TOTP_DIGITS,
    window: int = 1,
) -> bool:
    """Validate a TOTP code using the shared Base32 secret."""
    normalized_code = code.strip()
    if not normalized_code.isdigit() or len(normalized_code) != digits:
        return False

    normalized_secret = secret.strip().replace(" ", "")
    if not normalized_secret:
        return False

    try:
        padded_secret = normalized_secret
        remainder = len(padded_secret) % 8
        if remainder:
            padded_secret += "=" * (8 - remainder)
        decoded_secret = base64.b32decode(
            padded_secret.upper(),
            casefold=True,
        )
    except (binascii.Error, ValueError):
        return False

    timestamp = int(time.time() if now is None else now)
    counter = _totp_counter(timestamp, step_seconds=step_seconds)
    secret_fingerprint = _totp_secret_fingerprint(decoded_secret)
    last_counter_used = _LAST_ACCEPTED_TOTP_COUNTERS.get(secret_fingerprint)
    for delta in range(-window, window + 1):
        candidate_counter = counter + delta
        if last_counter_used is not None and candidate_counter <= last_counter_used:
            continue
        candidate = _hotp_value(
            decoded_secret,
            candidate_counter,
            digits=digits,
        )
        if hmac.compare_digest(candidate, normalized_code):
            _LAST_ACCEPTED_TOTP_COUNTERS[secret_fingerprint] = candidate_counter
            return True
    return False


def mint_signed_bearer_token(
    *,
    user_id: str,
    key_id: str,
    secret: str,
    expires_at: int,
    principal_type: Literal["user", "service"] = "user",
    display_name: str | None = None,
    scopes: list[str] | None = None,
    scope_enforced: bool = False,
) -> str:
    """Create a signed bearer token matching rust-core validation rules."""
    if not user_id.strip():
        message = "user_id must be non-empty"
        raise ValueError(message)
    if not key_id.strip():
        message = "key_id must be non-empty"
        raise ValueError(message)
    if not secret.strip():
        message = "secret must be non-empty"
        raise ValueError(message)

    payload: dict[str, object] = {
        "kid": key_id,
        "sub": user_id,
        "exp": int(expires_at),
        "principal_type": principal_type,
        "scope_enforced": scope_enforced,
    }
    if display_name:
        payload["display_name"] = display_name
    if scopes:
        payload["scopes"] = scopes

    payload_segment = _base64url_encode(
        json.dumps(payload, separators=(",", ":")).encode("utf-8"),
    )
    signature = hmac.new(
        secret.encode("utf-8"),
        payload_segment.encode("utf-8"),
        hashlib.sha256,
    ).digest()
    return (
        f"{DEFAULT_SIGNED_BEARER_VERSION}."
        f"{payload_segment}."
        f"{_base64url_encode(signature)}"
    )


def _build_auth_manager() -> AuthManager:
    """Create authentication manager from the current environment settings."""
    bootstrap_token: str | None = None
    if not _has_configured_bearer_credentials():
        bootstrap_token = os.environ.get("UGOITE_BOOTSTRAP_BEARER_TOKEN")
        if bootstrap_token is None:
            if _AUTH_MANAGER_CACHE.generated_bootstrap_token is None:
                _AUTH_MANAGER_CACHE.generated_bootstrap_token = secrets.token_urlsafe(
                    32,
                )
                logger.warning(
                    "No bearer credentials configured. "
                    "Generated one-time bootstrap token fingerprint=%s; "
                    "set UGOITE_BOOTSTRAP_BEARER_TOKEN or "
                    "UGOITE_AUTH_BEARER_TOKENS_JSON "
                    "for deterministic startup credentials.",
                    _token_fingerprint(_AUTH_MANAGER_CACHE.generated_bootstrap_token),
                )
            bootstrap_token = _AUTH_MANAGER_CACHE.generated_bootstrap_token

    return AuthManager(
        bootstrap_token=bootstrap_token,
        bootstrap_user_id=os.environ.get("UGOITE_BOOTSTRAP_USER_ID", "bootstrap-user"),
    )


def get_auth_manager() -> AuthManager:
    """Create and cache authentication manager from environment settings."""
    with _AUTH_MANAGER_CACHE_LOCK:
        now = time.monotonic()
        cache_entry = _AUTH_MANAGER_CACHE.entry
        if cache_entry is None:
            manager = _build_auth_manager()
            _AUTH_MANAGER_CACHE.entry = (now, manager)
            return manager

        cached_at, manager = cache_entry
        if now - cached_at >= AUTH_MANAGER_TTL_SECONDS:
            manager = _build_auth_manager()
            _AUTH_MANAGER_CACHE.entry = (now, manager)
        return manager


def clear_auth_manager_cache() -> None:
    """Clear cached auth runtime state for tests and dynamic config updates."""
    with _AUTH_MANAGER_CACHE_LOCK:
        _AUTH_MANAGER_CACHE.entry = None
        _AUTH_MANAGER_CACHE.generated_bootstrap_token = None
    _LAST_ACCEPTED_TOTP_COUNTERS.clear()


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
