"""Service account and API key management for automation access."""

from __future__ import annotations

import asyncio
import base64
import hashlib
import hmac
import json
import secrets
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any, cast

from . import _ugoite_core as _core
from .audit import AuditEventInput, append_audit_event

_core_any = cast("Any", _core)

_space_locks: dict[str, asyncio.Lock] = {}
_space_locks_guard = asyncio.Lock()

_ALL_SERVICE_SCOPES: frozenset[str] = frozenset(
    {
        "space_list",
        "space_read",
        "space_admin",
        "entry_read",
        "entry_write",
        "form_read",
        "form_write",
        "asset_read",
        "asset_write",
        "sql_read",
        "sql_write",
    },
)

_API_KEY_HASH_ALGORITHM = "pbkdf2_sha256_v1"
_API_KEY_HASH_ITERATIONS = 240_000


@dataclass(frozen=True)
class CreateServiceAccountInput:
    """Payload for creating a service account."""

    display_name: str
    scopes: list[str]
    created_by_user_id: str


@dataclass(frozen=True)
class CreateServiceAccountKeyInput:
    """Payload for creating a service-account API key."""

    service_account_id: str
    key_name: str
    created_by_user_id: str
    rotated_from: str | None = None


@dataclass(frozen=True)
class RotateServiceAccountKeyInput:
    """Payload for rotating a service-account API key."""

    service_account_id: str
    key_id: str
    rotated_by_user_id: str
    key_name: str | None = None


@dataclass(frozen=True)
class RevokeServiceAccountKeyInput:
    """Payload for revoking a service-account API key."""

    service_account_id: str
    key_id: str
    revoked_by_user_id: str


@dataclass(frozen=True)
class ServiceApiKeyAuthResult:
    """Resolved identity details for a service-account API key."""

    user_id: str
    service_account_id: str
    display_name: str
    key_id: str
    scopes: frozenset[str]


def _now_iso() -> str:
    return datetime.now(tz=UTC).isoformat().replace("+00:00", "Z")


async def _space_lock(space_id: str) -> asyncio.Lock:
    async with _space_locks_guard:
        existing = _space_locks.get(space_id)
        if existing is not None:
            return existing
        created = asyncio.Lock()
        _space_locks[space_id] = created
        return created


def _normalize_settings(space_meta: dict[str, Any]) -> dict[str, Any]:
    settings_obj = space_meta.get("settings")
    settings = settings_obj if isinstance(settings_obj, dict) else {}
    normalized = dict(settings)
    service_accounts_obj = normalized.get("service_accounts")
    normalized["service_accounts"] = (
        dict(service_accounts_obj) if isinstance(service_accounts_obj, dict) else {}
    )
    return normalized


def _normalize_scopes(scopes: list[str]) -> list[str]:
    normalized = [
        scope.strip() for scope in scopes if isinstance(scope, str) and scope.strip()
    ]
    deduped = sorted(set(normalized))
    if not deduped:
        msg = "service account scopes must not be empty"
        raise RuntimeError(msg)
    invalid = [scope for scope in deduped if scope not in _ALL_SERVICE_SCOPES]
    if invalid:
        msg = f"invalid service account scope(s): {', '.join(sorted(invalid))}"
        raise RuntimeError(msg)
    return deduped


def _new_service_account_id() -> str:
    return f"svc-{secrets.token_hex(8)}"


def _new_key_id() -> str:
    return f"sak-{secrets.token_hex(8)}"


def _new_secret() -> str:
    return f"ugsk_{secrets.token_urlsafe(32)}"


def _hash_api_key_secret(secret: str, salt: str) -> str:
    derived = hashlib.pbkdf2_hmac(
        "sha256",
        secret.encode("utf-8"),
        salt.encode("utf-8"),
        _API_KEY_HASH_ITERATIONS,
        dklen=32,
    )
    return base64.urlsafe_b64encode(derived).decode("ascii")


def _verify_api_key_secret(key_obj: dict[str, Any], secret: str) -> bool:
    key_hash = key_obj.get("secret_hash")
    if not isinstance(key_hash, str):
        return False

    hash_algorithm = key_obj.get("hash_algorithm")
    key_salt = key_obj.get("secret_salt")
    if hash_algorithm != _API_KEY_HASH_ALGORITHM:
        return False
    if not isinstance(key_salt, str) or not key_salt:
        return False

    expected = _hash_api_key_secret(secret, key_salt)
    return hmac.compare_digest(key_hash, expected)


def _key_public_view(key_obj: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": key_obj.get("id"),
        "name": key_obj.get("name"),
        "prefix": key_obj.get("prefix"),
        "created_at": key_obj.get("created_at"),
        "created_by_user_id": key_obj.get("created_by_user_id"),
        "revoked_at": key_obj.get("revoked_at"),
        "rotated_from": key_obj.get("rotated_from"),
        "last_used_at": key_obj.get("last_used_at"),
        "usage_count": key_obj.get("usage_count", 0),
    }


def _service_account_public_view(
    account_id: str,
    account_obj: dict[str, Any],
) -> dict[str, Any]:
    keys_obj = account_obj.get("keys")
    keys = keys_obj if isinstance(keys_obj, dict) else {}
    key_list = [
        _key_public_view(key_obj)
        for key_obj in keys.values()
        if isinstance(key_obj, dict)
    ]
    key_list.sort(key=lambda item: str(item.get("created_at") or ""), reverse=True)
    return {
        "id": account_id,
        "user_id": account_obj.get("user_id"),
        "display_name": account_obj.get("display_name"),
        "disabled": bool(account_obj.get("disabled", False)),
        "scopes": account_obj.get("scopes", []),
        "created_at": account_obj.get("created_at"),
        "created_by_user_id": account_obj.get("created_by_user_id"),
        "keys": key_list,
    }


async def list_service_accounts(
    storage_config: dict[str, str],
    space_id: str,
) -> list[dict[str, Any]]:
    """List service accounts and key metadata for a space."""
    space_meta_obj = await _core_any.get_space(storage_config, space_id)
    space_meta = cast("dict[str, Any]", space_meta_obj)
    settings = _normalize_settings(space_meta)
    accounts_obj = settings.get("service_accounts")
    accounts = accounts_obj if isinstance(accounts_obj, dict) else {}
    result = [
        _service_account_public_view(account_id, account_obj)
        for account_id, account_obj in accounts.items()
        if isinstance(account_id, str) and isinstance(account_obj, dict)
    ]
    result.sort(key=lambda item: str(item.get("created_at") or ""), reverse=True)
    return result


async def create_service_account(
    storage_config: dict[str, str],
    space_id: str,
    payload: CreateServiceAccountInput,
) -> dict[str, Any]:
    """Create a service account with explicit action scopes."""
    display_name = payload.display_name.strip()
    if not display_name:
        msg = "display_name must not be empty"
        raise RuntimeError(msg)

    created_by = payload.created_by_user_id.strip()
    if not created_by:
        msg = "created_by_user_id must not be empty"
        raise RuntimeError(msg)

    scopes = _normalize_scopes(payload.scopes)

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)
        accounts_obj = settings.get("service_accounts")
        accounts = accounts_obj if isinstance(accounts_obj, dict) else {}

        account_id = _new_service_account_id()
        user_id = f"service:{space_id}:{account_id}"
        account = {
            "id": account_id,
            "user_id": user_id,
            "display_name": display_name,
            "disabled": False,
            "scopes": scopes,
            "created_at": _now_iso(),
            "created_by_user_id": created_by,
            "keys": {},
        }
        accounts[account_id] = account
        settings["service_accounts"] = accounts

        await _core_any.patch_space(
            storage_config,
            space_id,
            json.dumps({"settings": settings}, separators=(",", ":"), sort_keys=True),
        )

    await append_audit_event(
        storage_config,
        space_id,
        AuditEventInput(
            action="service_account.create",
            actor_user_id=created_by,
            outcome="success",
            target_type="service_account",
            target_id=account_id,
            metadata={"scopes": scopes},
        ),
    )
    return _service_account_public_view(account_id, account)


async def create_service_account_key(
    storage_config: dict[str, str],
    space_id: str,
    payload: CreateServiceAccountKeyInput,
) -> dict[str, Any]:
    """Create a new service-account API key with one-time secret reveal."""
    service_account_id = payload.service_account_id.strip()
    key_name = payload.key_name.strip()
    created_by = payload.created_by_user_id.strip()
    if not service_account_id:
        msg = "service_account_id must not be empty"
        raise RuntimeError(msg)
    if not key_name:
        msg = "key_name must not be empty"
        raise RuntimeError(msg)
    if not created_by:
        msg = "created_by_user_id must not be empty"
        raise RuntimeError(msg)

    secret = _new_secret()
    secret_salt = secrets.token_urlsafe(16)
    secret_hash = _hash_api_key_secret(secret, secret_salt)
    key_id = _new_key_id()

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)
        accounts_obj = settings.get("service_accounts")
        accounts = accounts_obj if isinstance(accounts_obj, dict) else {}
        account_obj = accounts.get(service_account_id)
        if not isinstance(account_obj, dict):
            msg = f"Service account not found: {service_account_id}"
            raise RuntimeError(msg)

        keys_obj = account_obj.get("keys")
        keys = keys_obj if isinstance(keys_obj, dict) else {}
        key_payload = {
            "id": key_id,
            "name": key_name,
            "prefix": secret[:12],
            "secret_hash": secret_hash,
            "secret_salt": secret_salt,
            "hash_algorithm": _API_KEY_HASH_ALGORITHM,
            "created_at": _now_iso(),
            "created_by_user_id": created_by,
            "revoked_at": None,
            "rotated_from": payload.rotated_from,
            "last_used_at": None,
            "usage_count": 0,
        }
        keys[key_id] = key_payload
        account_obj["keys"] = keys
        accounts[service_account_id] = account_obj
        settings["service_accounts"] = accounts

        await _core_any.patch_space(
            storage_config,
            space_id,
            json.dumps({"settings": settings}, separators=(",", ":"), sort_keys=True),
        )

    await append_audit_event(
        storage_config,
        space_id,
        AuditEventInput(
            action="service_account.key.create",
            actor_user_id=created_by,
            outcome="success",
            target_type="service_account_key",
            target_id=key_id,
            metadata={"service_account_id": service_account_id},
        ),
    )

    return {
        "service_account_id": service_account_id,
        "key": _key_public_view(key_payload),
        "secret": secret,
    }


async def revoke_service_account_key(
    storage_config: dict[str, str],
    space_id: str,
    payload: RevokeServiceAccountKeyInput,
) -> dict[str, Any]:
    """Revoke a service-account API key immediately."""
    service_account_id = payload.service_account_id.strip()
    key_id = payload.key_id.strip()
    revoked_by = payload.revoked_by_user_id.strip()
    if not service_account_id:
        msg = "service_account_id must not be empty"
        raise RuntimeError(msg)
    if not key_id:
        msg = "key_id must not be empty"
        raise RuntimeError(msg)
    if not revoked_by:
        msg = "revoked_by_user_id must not be empty"
        raise RuntimeError(msg)

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)
        accounts_obj = settings.get("service_accounts")
        accounts = accounts_obj if isinstance(accounts_obj, dict) else {}
        account_obj = accounts.get(service_account_id)
        if not isinstance(account_obj, dict):
            msg = f"Service account not found: {service_account_id}"
            raise RuntimeError(msg)

        keys_obj = account_obj.get("keys")
        keys = keys_obj if isinstance(keys_obj, dict) else {}
        key_payload = keys.get(key_id)
        if not isinstance(key_payload, dict):
            msg = f"Service account key not found: {key_id}"
            raise RuntimeError(msg)

        if key_payload.get("revoked_at") is None:
            key_payload["revoked_at"] = _now_iso()
        keys[key_id] = key_payload
        account_obj["keys"] = keys
        accounts[service_account_id] = account_obj
        settings["service_accounts"] = accounts

        await _core_any.patch_space(
            storage_config,
            space_id,
            json.dumps({"settings": settings}, separators=(",", ":"), sort_keys=True),
        )

    await append_audit_event(
        storage_config,
        space_id,
        AuditEventInput(
            action="service_account.key.revoke",
            actor_user_id=revoked_by,
            outcome="success",
            target_type="service_account_key",
            target_id=key_id,
            metadata={"service_account_id": service_account_id},
        ),
    )

    return {
        "service_account_id": service_account_id,
        "key": _key_public_view(key_payload),
    }


async def rotate_service_account_key(
    storage_config: dict[str, str],
    space_id: str,
    payload: RotateServiceAccountKeyInput,
) -> dict[str, Any]:
    """Rotate an API key and return the new one-time secret."""
    await revoke_service_account_key(
        storage_config,
        space_id,
        RevokeServiceAccountKeyInput(
            service_account_id=payload.service_account_id,
            key_id=payload.key_id,
            revoked_by_user_id=payload.rotated_by_user_id,
        ),
    )
    created = await create_service_account_key(
        storage_config,
        space_id,
        CreateServiceAccountKeyInput(
            service_account_id=payload.service_account_id,
            key_name=payload.key_name or f"rotated-{payload.key_id}",
            created_by_user_id=payload.rotated_by_user_id,
            rotated_from=payload.key_id,
        ),
    )

    await append_audit_event(
        storage_config,
        space_id,
        AuditEventInput(
            action="service_account.key.rotate",
            actor_user_id=payload.rotated_by_user_id,
            outcome="success",
            target_type="service_account_key",
            target_id=payload.key_id,
            metadata={"service_account_id": payload.service_account_id},
        ),
    )
    return created


async def resolve_service_api_key(
    storage_config: dict[str, str],
    space_id: str,
    key_secret: str,
    *,
    request_method: str | None = None,
    request_path: str | None = None,
    request_id: str | None = None,
) -> ServiceApiKeyAuthResult:
    """Resolve service identity from a space-scoped API key and record usage."""
    secret = key_secret.strip()
    if not secret:
        msg = "Missing API key"
        raise RuntimeError(msg)
    hashed = secret

    matched_result: ServiceApiKeyAuthResult | None = None
    matched_usage_count: int | None = None
    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)
        accounts_obj = settings.get("service_accounts")
        accounts = accounts_obj if isinstance(accounts_obj, dict) else {}

        for service_account_id, account_obj in accounts.items():
            if not isinstance(service_account_id, str) or not isinstance(
                account_obj,
                dict,
            ):
                continue
            if bool(account_obj.get("disabled", False)):
                continue

            scopes_obj = account_obj.get("scopes")
            scopes = (
                [scope for scope in scopes_obj if isinstance(scope, str)]
                if isinstance(scopes_obj, list)
                else []
            )
            keys_obj = account_obj.get("keys")
            keys = keys_obj if isinstance(keys_obj, dict) else {}

            for key_id, key_obj in keys.items():
                if not isinstance(key_id, str) or not isinstance(key_obj, dict):
                    continue
                if not _verify_api_key_secret(key_obj, hashed):
                    continue

                if key_obj.get("revoked_at") is not None:
                    msg = "API key has been revoked"
                    raise RuntimeError(msg)

                key_obj["last_used_at"] = _now_iso()
                usage_count_obj = key_obj.get("usage_count", 0)
                try:
                    usage_count_int = int(usage_count_obj)
                except (TypeError, ValueError):
                    usage_count_int = 0
                key_obj["usage_count"] = usage_count_int + 1
                matched_usage_count = usage_count_int + 1
                keys[key_id] = key_obj
                account_obj["keys"] = keys
                accounts[service_account_id] = account_obj
                settings["service_accounts"] = accounts

                await _core_any.patch_space(
                    storage_config,
                    space_id,
                    json.dumps(
                        {"settings": settings},
                        separators=(",", ":"),
                        sort_keys=True,
                    ),
                )

                user_id_obj = account_obj.get("user_id")
                display_name_obj = account_obj.get("display_name")
                if not isinstance(user_id_obj, str):
                    user_id_obj = f"service:{space_id}:{service_account_id}"
                if not isinstance(display_name_obj, str):
                    display_name_obj = service_account_id
                matched_result = ServiceApiKeyAuthResult(
                    user_id=user_id_obj,
                    service_account_id=service_account_id,
                    display_name=display_name_obj,
                    key_id=key_id,
                    scopes=frozenset(scopes),
                )
                break
            if matched_result is not None:
                break

    if matched_result is None:
        msg = "Invalid API key"
        raise RuntimeError(msg)

    await append_audit_event(
        storage_config,
        space_id,
        AuditEventInput(
            action="service_account.key.use",
            actor_user_id=matched_result.user_id,
            outcome="success",
            target_type="service_account_key",
            target_id=matched_result.key_id,
            request_method=request_method,
            request_path=request_path,
            request_id=request_id,
            metadata={
                "service_account_id": matched_result.service_account_id,
                "usage_count": str(matched_usage_count or 1),
            },
        ),
    )

    return matched_result


__all__ = [
    "CreateServiceAccountInput",
    "CreateServiceAccountKeyInput",
    "RevokeServiceAccountKeyInput",
    "RotateServiceAccountKeyInput",
    "ServiceApiKeyAuthResult",
    "create_service_account",
    "create_service_account_key",
    "list_service_accounts",
    "resolve_service_api_key",
    "revoke_service_account_key",
    "rotate_service_account_key",
]
