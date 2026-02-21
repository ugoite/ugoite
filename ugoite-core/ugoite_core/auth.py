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
import time
from dataclasses import dataclass
from functools import lru_cache
from typing import Literal, NoReturn, cast

from .service_accounts import resolve_service_api_key

DEFAULT_UNAUTHORIZED_STATUS_CODE = 401
SIGNED_TOKEN_PARTS = 3
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
    value = getter(name)
    if isinstance(value, str):
        return value
    value = getter(name.lower())
    if isinstance(value, str):
        return value
    value = getter(name.upper())
    if isinstance(value, str):
        return value
    return None


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


@dataclass(frozen=True)
class _CredentialRecord:
    """Credential record resolved from configuration."""

    user_id: str
    principal_type: Literal["user", "service"]
    display_name: str | None
    key_id: str | None
    disabled: bool
    scopes: frozenset[str]
    scope_enforced: bool
    service_account_id: str | None


class AuthError(Exception):
    """Authentication error carrying status information."""

    def __init__(
        self,
        code: str,
        detail: str,
        status_code: int = DEFAULT_UNAUTHORIZED_STATUS_CODE,
    ) -> None:
        """Create an authentication exception with an error code."""
        super().__init__(detail)
        self.code = code
        self.detail = detail
        self.status_code = status_code


class _BearerTokenProvider:
    """Validates bearer credentials from signed or static tokens."""

    def __init__(
        self,
        static_tokens: dict[str, _CredentialRecord],
        signing_secrets: dict[str, str],
        active_kids: set[str],
        revoked_key_ids: set[str],
    ) -> None:
        self._static_tokens = static_tokens
        self._signing_secrets = signing_secrets
        self._active_kids = active_kids
        self._revoked_key_ids = revoked_key_ids

    def authenticate(self, token: str) -> RequestIdentity:
        token = token.strip()
        if not token:
            _raise_auth("missing_credentials", "Missing bearer token")

        if token.startswith("v1."):
            return self._authenticate_signed_token(token)

        record = self._static_tokens.get(token)
        if record is None:
            _raise_auth("invalid_credentials", "Invalid bearer token")
        if record.key_id and record.key_id in self._revoked_key_ids:
            _raise_auth("revoked_key", "Bearer token has been revoked")
        if record.disabled:
            _raise_auth("disabled_identity", "Principal is disabled")

        return RequestIdentity(
            user_id=record.user_id,
            principal_type=record.principal_type,
            display_name=record.display_name,
            auth_method="bearer",
            key_id=record.key_id,
            scopes=record.scopes,
            scope_enforced=record.scope_enforced,
            service_account_id=record.service_account_id,
        )

    def _authenticate_signed_token(self, token: str) -> RequestIdentity:
        payload_segment, payload_bytes, signature_bytes = self._decode_signed_segments(
            token,
        )
        payload = self._load_payload(payload_bytes)
        kid, secret = self._resolve_signing_secret(payload)
        self._verify_signed_signature(payload_segment, signature_bytes, secret)
        return self._identity_from_signed_payload(payload, kid)

    def _decode_signed_segments(self, token: str) -> tuple[str, bytes, bytes]:
        parts = token.split(".")
        if len(parts) != SIGNED_TOKEN_PARTS:
            _raise_auth("invalid_signature", "Malformed signed bearer token")

        _, payload_segment, signature_segment = parts
        try:
            payload_bytes = _urlsafe_b64decode(payload_segment)
            signature_bytes = _urlsafe_b64decode(signature_segment)
        except (ValueError, binascii.Error):
            _raise_auth("invalid_signature", "Malformed signed bearer token")
        return payload_segment, payload_bytes, signature_bytes

    def _load_payload(self, payload_bytes: bytes) -> dict[str, object]:
        try:
            payload = json.loads(payload_bytes.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            error_code = "invalid_signature"
            error_detail = "Invalid signed token payload"
            raise AuthError(error_code, error_detail) from exc

        if not isinstance(payload, dict):
            _raise_auth("invalid_signature", "Invalid signed token payload")
        return payload

    def _resolve_signing_secret(self, payload: dict[str, object]) -> tuple[str, str]:
        kid = payload.get("kid")
        if not isinstance(kid, str) or not kid:
            _raise_auth("invalid_signature", "Signed token missing key id")
        if self._active_kids and kid not in self._active_kids:
            _raise_auth("revoked_key", "Token signed by inactive key")
        if kid in self._revoked_key_ids:
            _raise_auth("revoked_key", "Token key id has been revoked")

        secret = self._signing_secrets.get(kid)
        if secret is None:
            _raise_auth("invalid_signature", "Unknown token signing key")
        return kid, secret

    def _verify_signed_signature(
        self,
        payload_segment: str,
        signature_bytes: bytes,
        secret: str,
    ) -> None:
        expected_signature = hmac.new(
            secret.encode("utf-8"),
            payload_segment.encode("utf-8"),
            hashlib.sha256,
        ).digest()

        if not hmac.compare_digest(expected_signature, signature_bytes):
            _raise_auth("invalid_signature", "Invalid bearer token signature")

    def _identity_from_signed_payload(
        self,
        payload: dict[str, object],
        kid: str,
    ) -> RequestIdentity:
        exp = payload.get("exp")
        if not isinstance(exp, int | float):
            _raise_auth("invalid_credentials", "Signed token missing exp")
        if exp < time.time():
            _raise_auth("expired_token", "Bearer token has expired")

        user_id = payload.get("sub")
        if not isinstance(user_id, str) or not user_id:
            _raise_auth("invalid_credentials", "Signed token missing subject")
        disabled = payload.get("disabled", False)
        if disabled:
            _raise_auth("disabled_identity", "Principal is disabled")

        principal_type_obj = payload.get("principal_type", "user")
        if principal_type_obj not in {"user", "service"}:
            _raise_auth("invalid_credentials", "Invalid principal type")
        principal_type = cast("Literal['user', 'service']", principal_type_obj)

        display_name = payload.get("display_name")
        if display_name is not None and not isinstance(display_name, str):
            _raise_auth("invalid_credentials", "Invalid display name")

        scopes_obj = payload.get("scopes")
        scopes = (
            frozenset(scope for scope in scopes_obj if isinstance(scope, str) and scope)
            if isinstance(scopes_obj, list)
            else frozenset()
        )
        service_account_id_obj = payload.get("service_account_id")
        service_account_id = (
            service_account_id_obj if isinstance(service_account_id_obj, str) else None
        )

        return RequestIdentity(
            user_id=user_id,
            principal_type=principal_type,
            display_name=display_name,
            auth_method="bearer",
            key_id=kid,
            scopes=scopes,
            scope_enforced=bool(payload.get("scope_enforced", False)),
            service_account_id=service_account_id,
        )


class _ApiKeyProvider:
    """Validates API key credentials."""

    def __init__(
        self,
        api_keys: dict[str, _CredentialRecord],
        revoked_key_ids: set[str],
    ) -> None:
        self._api_keys = api_keys
        self._revoked_key_ids = revoked_key_ids

    def authenticate(self, key_value: str) -> RequestIdentity:
        key_value = key_value.strip()
        if not key_value:
            _raise_auth("missing_credentials", "Missing API key")

        record = self._api_keys.get(key_value)
        if record is None:
            _raise_auth("invalid_credentials", "Invalid API key")
        if record.key_id and record.key_id in self._revoked_key_ids:
            _raise_auth("revoked_key", "API key has been revoked")
        if record.disabled:
            _raise_auth("disabled_identity", "Principal is disabled")

        return RequestIdentity(
            user_id=record.user_id,
            principal_type=record.principal_type,
            display_name=record.display_name,
            auth_method="api_key",
            key_id=record.key_id,
            scopes=record.scopes,
            scope_enforced=record.scope_enforced,
            service_account_id=record.service_account_id,
        )


class AuthManager:
    """Coordinates authentication providers for incoming requests."""

    def __init__(
        self,
        bearer_provider: _BearerTokenProvider,
        api_key_provider: _ApiKeyProvider,
    ) -> None:
        """Initialize auth providers for bearer and API key credentials."""
        self._bearer_provider = bearer_provider
        self._api_key_provider = api_key_provider

    def authenticate_headers(self, headers: dict[str, str] | object) -> RequestIdentity:
        """Resolve request identity using bearer or API key credentials."""
        authorization = _header_value(headers, "authorization")
        if authorization:
            parts = authorization.split(" ", 1)
            if len(parts) != AUTH_HEADER_PARTS or parts[0].lower() != "bearer":
                _raise_auth(
                    "invalid_credentials",
                    "Authorization header must use Bearer scheme",
                )
            return self._bearer_provider.authenticate(parts[1])

        api_key = _header_value(headers, "x-api-key")
        if api_key:
            return self._api_key_provider.authenticate(api_key)

        _raise_auth(
            "missing_credentials",
            (
                "Authentication required. Provide Authorization: Bearer <token> "
                "or X-API-Key."
            ),
        )


def _urlsafe_b64decode(data: str) -> bytes:
    padding = "=" * (-len(data) % 4)
    return base64.urlsafe_b64decode(data + padding)


def _parse_json_map(raw: str | None) -> dict[str, dict[str, object]]:
    if not raw:
        return {}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    if not isinstance(parsed, dict):
        return {}
    return {
        key: value
        for key, value in parsed.items()
        if isinstance(key, str) and isinstance(value, dict)
    }


def _parse_key_value_map(raw: str | None) -> dict[str, str]:
    if not raw:
        return {}
    result: dict[str, str] = {}
    for pair in raw.split(","):
        item = pair.strip()
        if not item:
            continue
        key, sep, value = item.partition(":")
        if not sep:
            continue
        k = key.strip()
        v = value.strip()
        if k and v:
            result[k] = v
    return result


def _parse_record_map(raw: str | None) -> dict[str, _CredentialRecord]:
    records: dict[str, _CredentialRecord] = {}
    for credential, data in _parse_json_map(raw).items():
        user_id = data.get("user_id")
        if not isinstance(user_id, str) or not user_id:
            continue
        principal_type_obj = data.get("principal_type", "user")
        if principal_type_obj not in {"user", "service"}:
            continue
        principal_type = cast("Literal['user', 'service']", principal_type_obj)
        display_name = data.get("display_name")
        if display_name is not None and not isinstance(display_name, str):
            display_name = None
        key_id = data.get("key_id")
        if key_id is not None and not isinstance(key_id, str):
            key_id = None
        disabled = bool(data.get("disabled", False))
        scopes_obj = data.get("scopes")
        scopes = (
            frozenset(scope for scope in scopes_obj if isinstance(scope, str) and scope)
            if isinstance(scopes_obj, list)
            else frozenset()
        )
        service_account_id = data.get("service_account_id")
        if service_account_id is not None and not isinstance(service_account_id, str):
            service_account_id = None
        records[credential] = _CredentialRecord(
            user_id=user_id,
            principal_type=principal_type,
            display_name=display_name,
            key_id=key_id,
            disabled=disabled,
            scopes=scopes,
            scope_enforced=bool(data.get("scope_enforced", False)),
            service_account_id=service_account_id,
        )
    return records


def _parse_string_set(raw: str | None) -> set[str]:
    if not raw:
        return set()
    return {item.strip() for item in raw.split(",") if item.strip()}


@lru_cache(maxsize=1)
def get_auth_manager() -> AuthManager:
    """Create and cache authentication manager from environment settings."""
    bearer_tokens = _parse_record_map(os.environ.get("UGOITE_AUTH_BEARER_TOKENS_JSON"))
    if not bearer_tokens:
        bootstrap_token = os.environ.get("UGOITE_BOOTSTRAP_BEARER_TOKEN")
        if bootstrap_token is None:
            bootstrap_token = secrets.token_urlsafe(32)
            logger.warning(
                "No bearer credentials configured. "
                "Generated one-time bootstrap token %s; "
                "set UGOITE_BOOTSTRAP_BEARER_TOKEN or UGOITE_AUTH_BEARER_TOKENS_JSON "
                "for deterministic startup credentials.",
                bootstrap_token,
            )
        bearer_tokens[bootstrap_token] = _CredentialRecord(
            user_id=os.environ.get("UGOITE_BOOTSTRAP_USER_ID", "bootstrap-user"),
            principal_type="user",
            display_name="Local Bootstrap User",
            key_id="bootstrap",
            disabled=False,
            scopes=frozenset(),
            scope_enforced=False,
            service_account_id=None,
        )

    api_keys = _parse_record_map(os.environ.get("UGOITE_AUTH_API_KEYS_JSON"))
    for key, user_id in _parse_key_value_map(
        os.environ.get("UGOITE_AUTH_API_KEYS"),
    ).items():
        api_keys[key] = _CredentialRecord(
            user_id=user_id,
            principal_type="service",
            display_name=None,
            key_id=None,
            disabled=False,
            scopes=frozenset(),
            scope_enforced=False,
            service_account_id=None,
        )

    signing_secrets = _parse_key_value_map(os.environ.get("UGOITE_AUTH_BEARER_SECRETS"))
    active_kids = _parse_string_set(os.environ.get("UGOITE_AUTH_BEARER_ACTIVE_KIDS"))
    revoked_keys = _parse_string_set(os.environ.get("UGOITE_AUTH_REVOKED_KEY_IDS"))

    return AuthManager(
        bearer_provider=_BearerTokenProvider(
            static_tokens=bearer_tokens,
            signing_secrets=signing_secrets,
            active_kids=active_kids,
            revoked_key_ids=revoked_keys,
        ),
        api_key_provider=_ApiKeyProvider(
            api_keys=api_keys,
            revoked_key_ids=revoked_keys,
        ),
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


def auth_headers_from_environment() -> dict[str, str]:
    """Build outbound auth headers for CLI/frontend calls to backend APIs."""
    bearer_token = os.environ.get("UGOITE_AUTH_BEARER_TOKEN")
    if bearer_token:
        return {"Authorization": f"Bearer {bearer_token}"}

    api_key = os.environ.get("UGOITE_AUTH_API_KEY")
    if api_key:
        return {"X-API-Key": api_key}

    return {}
