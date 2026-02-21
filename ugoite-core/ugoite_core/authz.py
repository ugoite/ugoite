"""Authorization policy evaluation shared across backend/CLI adapters."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Literal, cast

from . import _ugoite_core as _core

if TYPE_CHECKING:
    from .auth import RequestIdentity

_core_any = cast("Any", _core)

RoleName = Literal["owner", "admin", "editor", "viewer", "service"]
ActionName = Literal[
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
]

_VALID_ROLES: set[str] = {"owner", "admin", "editor", "viewer", "service"}

_ROLE_PERMISSIONS: dict[RoleName, set[ActionName]] = {
    "owner": {
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
    "admin": {
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
    "editor": {
        "space_list",
        "space_read",
        "entry_read",
        "entry_write",
        "form_read",
        "form_write",
        "asset_read",
        "asset_write",
        "sql_read",
        "sql_write",
    },
    "viewer": {
        "space_list",
        "space_read",
        "entry_read",
        "form_read",
        "asset_read",
        "sql_read",
    },
    "service": {
        "space_list",
        "space_read",
        "entry_read",
        "entry_write",
        "form_read",
        "asset_read",
        "asset_write",
        "sql_read",
        "sql_write",
    },
}


@dataclass(frozen=True)
class AuthorizationError(Exception):
    """Raised when an authenticated principal is not authorized."""

    code: str
    detail: str
    action: ActionName
    status_code: int = 403


@dataclass(frozen=True)
class AccessContext:
    """Resolved authorization context for a user in a space."""

    space_id: str
    user_id: str
    role: RoleName
    groups: frozenset[str]
    form_acls: dict[str, dict[str, Any]]


def _normalized_role(raw: object, fallback: RoleName) -> RoleName:
    if isinstance(raw, str) and raw in _VALID_ROLES:
        return cast("RoleName", raw)
    return fallback


def _default_user_role() -> RoleName:
    return _normalized_role(os.environ.get("UGOITE_AUTHZ_DEFAULT_USER_ROLE"), "editor")


def _default_service_role() -> RoleName:
    return _normalized_role(
        os.environ.get("UGOITE_AUTHZ_DEFAULT_SERVICE_ROLE"),
        "service",
    )


def _permissions_for_role(role: RoleName) -> set[ActionName]:
    return _ROLE_PERMISSIONS.get(role, set())


def _parse_groups_map(raw: str | None) -> dict[str, dict[str, list[str]]]:
    if not raw:
        return {}
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    if not isinstance(data, dict):
        return {}

    result: dict[str, dict[str, list[str]]] = {}
    for space_id, users in data.items():
        if not isinstance(space_id, str) or not isinstance(users, dict):
            continue
        normalized_users: dict[str, list[str]] = {}
        for user_id, groups in users.items():
            if not isinstance(user_id, str) or not isinstance(groups, list):
                continue
            normalized = [item for item in groups if isinstance(item, str) and item]
            if normalized:
                normalized_users[user_id] = normalized
        if normalized_users:
            result[space_id] = normalized_users
    return result


def _groups_from_space_meta(
    space_id: str,
    user_id: str,
    space_meta: dict[str, Any],
) -> frozenset[str]:
    groups: set[str] = set()
    settings = space_meta.get("settings")
    settings_map = settings if isinstance(settings, dict) else {}

    user_groups = space_meta.get("user_groups")
    if isinstance(user_groups, dict):
        values = user_groups.get(user_id)
        if isinstance(values, list):
            groups.update(item for item in values if isinstance(item, str) and item)
    settings_groups = settings_map.get("user_groups")
    if isinstance(settings_groups, dict):
        values = settings_groups.get(user_id)
        if isinstance(values, list):
            groups.update(item for item in values if isinstance(item, str) and item)

    configured = _parse_groups_map(os.environ.get("UGOITE_AUTHZ_USER_GROUPS_JSON"))
    groups.update(configured.get(space_id, {}).get(user_id, []))
    return frozenset(groups)


def _resolve_role(space_meta: dict[str, Any], identity: RequestIdentity) -> RoleName:
    settings = space_meta.get("settings")
    settings_map = settings if isinstance(settings, dict) else {}

    if identity.principal_type == "service":
        return _default_service_role()

    owner_user_id = space_meta.get("owner_user_id")
    if not isinstance(owner_user_id, str):
        owner_user_id = settings_map.get("owner_user_id")
    if isinstance(owner_user_id, str) and owner_user_id == identity.user_id:
        return "owner"

    admin_user_ids = space_meta.get("admin_user_ids")
    if not isinstance(admin_user_ids, list):
        admin_user_ids = settings_map.get("admin_user_ids")
    if isinstance(admin_user_ids, list) and identity.user_id in admin_user_ids:
        return "admin"

    member_roles = space_meta.get("member_roles")
    if isinstance(member_roles, dict):
        explicit = member_roles.get(identity.user_id)
        if isinstance(explicit, str) and explicit in _VALID_ROLES:
            return cast("RoleName", explicit)

    settings_roles = settings_map.get("member_roles")
    if isinstance(settings_roles, dict):
        explicit = settings_roles.get(identity.user_id)
        if isinstance(explicit, str) and explicit in _VALID_ROLES:
            return cast("RoleName", explicit)

    return _default_user_role()


async def resolve_access_context(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
) -> AccessContext:
    """Resolve role/group context for a principal in a space."""
    get_space_raw = getattr(_core, "get_space_raw", None)
    if callable(get_space_raw):
        space_meta_obj = await get_space_raw(storage_config, space_id)
    else:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
    space_meta = cast("dict[str, Any]", space_meta_obj)
    role = _resolve_role(space_meta, identity)
    groups = _groups_from_space_meta(space_id, identity.user_id, space_meta)
    settings = space_meta.get("settings")
    settings_map = settings if isinstance(settings, dict) else {}
    form_acls_obj = settings_map.get("form_acls")
    form_acls = form_acls_obj if isinstance(form_acls_obj, dict) else {}

    return AccessContext(
        space_id=space_id,
        user_id=identity.user_id,
        role=role,
        groups=groups,
        form_acls={
            key: value
            for key, value in form_acls.items()
            if isinstance(key, str) and isinstance(value, dict)
        },
    )


def _deny(action: ActionName, detail: str) -> None:
    raise AuthorizationError(
        code="forbidden",
        detail=detail,
        action=action,
    )


async def require_space_action(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    action: ActionName,
) -> AccessContext:
    """Require role-based permission for a space-scoped action."""
    access = await resolve_access_context(storage_config, space_id, identity)
    if action in _permissions_for_role(access.role):
        return access
    _deny(
        action,
        (
            f"Principal '{identity.user_id}' with role '{access.role}' "
            f"is not allowed to perform '{action}' in space '{space_id}'."
        ),
    )
    message = "unreachable"
    raise RuntimeError(message)


def _form_name_from_markdown(markdown: str) -> str | None:
    extracted = _core_any.extract_properties(markdown)
    if isinstance(extracted, dict):
        raw = extracted.get("form")
        if isinstance(raw, str) and raw.strip():
            return raw.strip()
    return None


def form_name_from_entry(entry: dict[str, Any]) -> str | None:
    """Resolve form name from an entry payload."""
    form = entry.get("form")
    if isinstance(form, str) and form.strip():
        return form.strip()

    properties = entry.get("properties")
    if isinstance(properties, dict):
        form = properties.get("form")
        if isinstance(form, str) and form.strip():
            return form.strip()

    markdown = entry.get("markdown")
    if isinstance(markdown, str) and markdown.strip():
        return _form_name_from_markdown(markdown)

    content = entry.get("content")
    if isinstance(content, str) and content.strip():
        return _form_name_from_markdown(content)

    return None


def _principal_matches(
    principal: dict[str, Any],
    identity: RequestIdentity,
    groups: frozenset[str],
) -> bool:
    kind = principal.get("kind")
    principal_id = principal.get("id")
    if not isinstance(kind, str) or not isinstance(principal_id, str):
        return False

    if kind == "user":
        return principal_id == identity.user_id
    if kind == "user_group":
        return principal_id in groups
    return False


def _check_form_acl(
    form_def: dict[str, Any],
    acl_field: str,
    identity: RequestIdentity,
    access: AccessContext,
    action: ActionName,
) -> None:
    principals = form_def.get(acl_field)
    if principals is None:
        return
    if access.role in {"owner", "admin"}:
        return
    if not isinstance(principals, list):
        return
    for principal in principals:
        if isinstance(principal, dict) and _principal_matches(
            principal,
            identity,
            access.groups,
        ):
            return
    _deny(
        action,
        (
            f"Principal '{identity.user_id}' is not allowed by '{acl_field}' "
            f"for form '{form_def.get('name', '<unknown>')}'."
        ),
    )


async def require_form_read(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    form_name: str,
) -> AccessContext:
    """Require read access to a form using role + ACL checks."""
    access = await require_space_action(storage_config, space_id, identity, "form_read")
    form_def_obj = await _core_any.get_form(storage_config, space_id, form_name)
    form_def = cast("dict[str, Any]", form_def_obj)
    effective_form = dict(form_def)
    effective_form.setdefault("name", form_name)
    if "read_principals" not in effective_form:
        acl = access.form_acls.get(form_name)
        if isinstance(acl, dict) and "read_principals" in acl:
            effective_form["read_principals"] = acl.get("read_principals")
    _check_form_acl(
        effective_form,
        "read_principals",
        identity,
        access,
        "form_read",
    )
    return access


async def require_form_write(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    form_name: str,
) -> AccessContext:
    """Require write access to a form using role + ACL checks."""
    access = await require_space_action(
        storage_config,
        space_id,
        identity,
        "entry_write",
    )
    form_def_obj = await _core_any.get_form(storage_config, space_id, form_name)
    form_def = cast("dict[str, Any]", form_def_obj)
    effective_form = dict(form_def)
    effective_form.setdefault("name", form_name)
    if "write_principals" not in effective_form:
        acl = access.form_acls.get(form_name)
        if isinstance(acl, dict) and "write_principals" in acl:
            effective_form["write_principals"] = acl.get("write_principals")
    _check_form_acl(
        effective_form,
        "write_principals",
        identity,
        access,
        "entry_write",
    )
    return access


async def require_markdown_write(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    markdown: str,
) -> AccessContext:
    """Require write access for markdown payload based on its form."""
    form_name = _form_name_from_markdown(markdown)
    if not form_name:
        return await require_space_action(
            storage_config,
            space_id,
            identity,
            "entry_write",
        )
    return await require_form_write(storage_config, space_id, identity, form_name)


async def require_entry_read(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    entry: dict[str, Any],
) -> AccessContext:
    """Require read access for an entry based on its form."""
    form_name = form_name_from_entry(entry)
    if not form_name:
        return await require_space_action(
            storage_config,
            space_id,
            identity,
            "entry_read",
        )
    return await require_form_read(storage_config, space_id, identity, form_name)


async def require_entry_write(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    entry: dict[str, Any],
) -> AccessContext:
    """Require write access for an existing entry based on its form."""
    form_name = form_name_from_entry(entry)
    if not form_name:
        return await require_space_action(
            storage_config,
            space_id,
            identity,
            "entry_write",
        )
    return await require_form_write(storage_config, space_id, identity, form_name)


async def filter_readable_entries(
    storage_config: dict[str, str],
    space_id: str,
    identity: RequestIdentity,
    entries: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Filter entries by read authorization (deny-by-default on ACL mismatch)."""
    filtered: list[dict[str, Any]] = []
    for entry in entries:
        try:
            await require_entry_read(storage_config, space_id, identity, entry)
        except AuthorizationError:
            continue
        except RuntimeError:
            continue
        filtered.append(entry)
    return filtered


__all__ = [
    "AccessContext",
    "ActionName",
    "AuthorizationError",
    "RoleName",
    "filter_readable_entries",
    "form_name_from_entry",
    "require_entry_read",
    "require_entry_write",
    "require_form_read",
    "require_form_write",
    "require_markdown_write",
    "require_space_action",
    "resolve_access_context",
]
