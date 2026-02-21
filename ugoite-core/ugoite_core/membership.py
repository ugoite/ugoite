"""Space membership and collaboration primitives for Milestone 4 Phase 3."""

from __future__ import annotations

import asyncio
import hashlib
import json
import secrets
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any, Protocol, cast

from . import _ugoite_core as _core

_core_any = cast("Any", _core)

MemberRole = str
MemberState = str
_VALID_MEMBER_ROLES: set[str] = {"owner", "admin", "editor", "viewer"}
_MUTABLE_MEMBER_ROLES: set[str] = {"admin", "editor", "viewer"}

_space_locks: dict[str, asyncio.Lock] = {}
_space_locks_guard = asyncio.Lock()


class InvitationDeliveryProvider(Protocol):
    """Provider-agnostic invitation dispatch contract."""

    async def issue_invitation(
        self,
        *,
        space_id: str,
        invitation: dict[str, Any],
    ) -> dict[str, Any]:
        """Dispatch invitation to an external channel and return metadata."""


class TokenOnlyInvitationProvider:
    """Default provider returning token metadata without external delivery."""

    async def issue_invitation(
        self,
        *,
        space_id: str,
        invitation: dict[str, Any],
    ) -> dict[str, Any]:
        """Return token metadata without sending to an external provider."""
        _ = space_id
        return {
            "channel": "token",
            "token": invitation.get("token"),
        }


@dataclass(frozen=True)
class InviteMemberInput:
    """Payload for invitation creation."""

    user_id: str
    role: str
    invited_by_user_id: str
    email: str | None = None
    expires_in_seconds: int = 7 * 24 * 60 * 60


@dataclass(frozen=True)
class AcceptInvitationInput:
    """Payload for invitation acceptance."""

    token: str
    accepted_by_user_id: str


@dataclass(frozen=True)
class UpdateMemberRoleInput:
    """Payload for role update."""

    member_user_id: str
    role: str
    changed_by_user_id: str


@dataclass(frozen=True)
class RevokeMemberInput:
    """Payload for member revocation."""

    member_user_id: str
    revoked_by_user_id: str


def _now_iso() -> str:
    return datetime.now(tz=UTC).isoformat().replace("+00:00", "Z")


def _is_valid_member_role(role: object) -> bool:
    return isinstance(role, str) and role in _VALID_MEMBER_ROLES


def _is_mutable_member_role(role: object) -> bool:
    return isinstance(role, str) and role in _MUTABLE_MEMBER_ROLES


def _normalize_settings(space_meta: dict[str, Any]) -> dict[str, Any]:
    settings_obj = space_meta.get("settings")
    settings = settings_obj if isinstance(settings_obj, dict) else {}
    normalized = dict(settings)
    members = normalized.get("members")
    normalized["members"] = members if isinstance(members, dict) else {}
    invitations = normalized.get("invitations")
    normalized["invitations"] = invitations if isinstance(invitations, dict) else {}
    version = normalized.get("membership_version")
    normalized["membership_version"] = version if isinstance(version, int) else 0
    return normalized


def _increment_membership_version(settings: dict[str, Any]) -> None:
    current = settings.get("membership_version", 0)
    settings["membership_version"] = int(current) + 1


def _active_member_roles(settings: dict[str, Any]) -> dict[str, str]:
    roles: dict[str, str] = {}
    members = settings.get("members")
    if not isinstance(members, dict):
        return roles
    for user_id, member in members.items():
        if not isinstance(user_id, str) or not isinstance(member, dict):
            continue
        state = member.get("state")
        role = member.get("role")
        if state == "active" and _is_valid_member_role(role):
            roles[user_id] = cast("str", role)
    return roles


def _owner_user_id(space_meta: dict[str, Any], settings: dict[str, Any]) -> str | None:
    owner_obj = space_meta.get("owner_user_id")
    if isinstance(owner_obj, str):
        return owner_obj
    settings_owner = settings.get("owner_user_id")
    if isinstance(settings_owner, str):
        return settings_owner
    return None


def _legacy_maps(
    *,
    space_meta: dict[str, Any],
    settings: dict[str, Any],
) -> tuple[str | None, list[str], dict[str, str]]:
    owner_user_id = _owner_user_id(space_meta, settings)
    active_roles = _active_member_roles(settings)

    member_roles = {
        user_id: role
        for user_id, role in active_roles.items()
        if role in _MUTABLE_MEMBER_ROLES
    }

    admin_user_ids_obj = settings.get("admin_user_ids")
    admin_user_ids: list[str] = []
    if isinstance(admin_user_ids_obj, list):
        admin_user_ids.extend(
            item for item in admin_user_ids_obj if isinstance(item, str)
        )

    admin_user_ids.extend(
        user_id for user_id, role in active_roles.items() if role == "admin"
    )

    if owner_user_id:
        admin_user_ids.append(owner_user_id)

    deduped_admin_ids = sorted(set(admin_user_ids))
    return owner_user_id, deduped_admin_ids, member_roles


async def _space_lock(space_id: str) -> asyncio.Lock:
    async with _space_locks_guard:
        existing = _space_locks.get(space_id)
        if existing is not None:
            return existing
        created = asyncio.Lock()
        _space_locks[space_id] = created
        return created


async def _patch_settings(
    storage_config: dict[str, str],
    space_id: str,
    space_meta: dict[str, Any],
    settings: dict[str, Any],
) -> None:
    owner_user_id, admin_user_ids, member_roles = _legacy_maps(
        space_meta=space_meta,
        settings=settings,
    )
    settings["admin_user_ids"] = admin_user_ids
    settings["member_roles"] = member_roles
    if owner_user_id:
        settings["owner_user_id"] = owner_user_id

    patch: dict[str, Any] = {
        "settings": settings,
        "admin_user_ids": admin_user_ids,
        "member_roles": member_roles,
    }
    if owner_user_id:
        patch["owner_user_id"] = owner_user_id

    await _core_any.patch_space(storage_config, space_id, json.dumps(patch))


def _audit_event(
    *,
    action: str,
    actor_user_id: str,
    space_id: str,
    target_user_id: str | None = None,
    extra: dict[str, Any] | None = None,
) -> dict[str, Any]:
    event = {
        "action": action,
        "actor_user_id": actor_user_id,
        "space_id": space_id,
        "target_user_id": target_user_id,
        "timestamp": _now_iso(),
    }
    if extra:
        event.update(extra)
    return event


def _parse_expiry(expires_at: object) -> datetime | None:
    if not isinstance(expires_at, str):
        return None
    try:
        return datetime.fromisoformat(expires_at)
    except ValueError as exc:
        msg = "Invitation token expiry is malformed"
        raise TypeError(msg) from exc


def _build_member_record(
    *,
    user_id: str,
    role: str,
    invited_by: str,
    invited_at: str,
    state: str,
) -> dict[str, Any]:
    return {
        "user_id": user_id,
        "role": role,
        "state": state,
        "invited_by": invited_by,
        "invited_at": invited_at,
        "revoked_at": None,
    }


def _token_hash(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


async def list_members(
    storage_config: dict[str, str],
    space_id: str,
) -> list[dict[str, Any]]:
    """Return all member records for a space."""
    space_meta_obj = await _core_any.get_space(storage_config, space_id)
    space_meta = cast("dict[str, Any]", space_meta_obj)
    settings = _normalize_settings(space_meta)
    members = settings.get("members")
    if not isinstance(members, dict):
        return []

    results: list[dict[str, Any]] = []
    for user_id, member in members.items():
        if isinstance(user_id, str) and isinstance(member, dict):
            normalized = dict(member)
            normalized.setdefault("user_id", user_id)
            results.append(normalized)
    results.sort(key=lambda item: str(item.get("user_id", "")))
    return results


async def create_invitation(
    storage_config: dict[str, str],
    space_id: str,
    payload: InviteMemberInput,
) -> dict[str, Any]:
    """Create a member invitation and transition member state to invited."""
    if not payload.user_id.strip():
        msg = "invited_user_id must not be empty"
        raise RuntimeError(msg)
    if not _is_mutable_member_role(payload.role):
        msg = "role must be one of admin/editor/viewer"
        raise RuntimeError(msg)

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)

        members = settings["members"]
        current = members.get(payload.user_id)
        if isinstance(current, dict) and current.get("state") == "active":
            msg = f"Member already active: {payload.user_id}"
            raise RuntimeError(msg)

        token = secrets.token_urlsafe(24)
        invited_at = _now_iso()
        expires_seconds = max(60, payload.expires_in_seconds)
        expires_at = (
            datetime.now(tz=UTC) + timedelta(seconds=expires_seconds)
        ).isoformat()

        token_hash = _token_hash(token)
        invitation_id = secrets.token_urlsafe(12)
        invitation = {
            "id": invitation_id,
            "token_hash": token_hash,
            "user_id": payload.user_id,
            "role": payload.role,
            "email": payload.email,
            "state": "pending",
            "invited_by": payload.invited_by_user_id,
            "invited_at": invited_at,
            "expires_at": expires_at,
        }
        invitations = settings["invitations"]
        invitations[invitation_id] = invitation

        members[payload.user_id] = _build_member_record(
            user_id=payload.user_id,
            role=payload.role,
            invited_by=payload.invited_by_user_id,
            invited_at=invited_at,
            state="invited",
        )

        _increment_membership_version(settings)
        await _patch_settings(storage_config, space_id, space_meta, settings)

    response_invitation = dict(invitation)
    response_invitation["token"] = token

    delivery = await TokenOnlyInvitationProvider().issue_invitation(
        space_id=space_id,
        invitation=response_invitation,
    )
    event = _audit_event(
        action="member.invite",
        actor_user_id=payload.invited_by_user_id,
        space_id=space_id,
        target_user_id=payload.user_id,
        extra={"role": payload.role},
    )
    return {
        "invitation": response_invitation,
        "delivery": delivery,
        "audit_event": event,
    }


async def accept_invitation(
    storage_config: dict[str, str],
    space_id: str,
    payload: AcceptInvitationInput,
) -> dict[str, Any]:
    """Accept an invitation token and activate member access."""
    if not payload.token.strip():
        msg = "token must not be empty"
        raise RuntimeError(msg)

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)

        invitations = settings["invitations"]
        invitation_key: str | None = None
        invitation_obj: dict[str, Any] | None = None
        requested_hash = _token_hash(payload.token)
        for key, candidate in invitations.items():
            if not isinstance(key, str) or not isinstance(candidate, dict):
                continue
            candidate_hash = candidate.get("token_hash")
            if isinstance(candidate_hash, str) and candidate_hash == requested_hash:
                invitation_key = key
                invitation_obj = candidate
                break

        if invitation_key is None or invitation_obj is None:
            msg = "Invitation token not found"
            raise RuntimeError(msg)
        if invitation_obj.get("state") != "pending":
            msg = "Invitation token is not pending"
            raise RuntimeError(msg)

        invited_user = invitation_obj.get("user_id")
        if not isinstance(invited_user, str):
            msg = "Invitation token user is malformed"
            raise TypeError(msg)
        if invited_user != payload.accepted_by_user_id:
            msg = "Invitation token is not valid for this user"
            raise RuntimeError(msg)

        expiry = _parse_expiry(invitation_obj.get("expires_at"))
        if expiry and expiry < datetime.now(tz=UTC):
            invitation_obj["state"] = "expired"
            invitations[invitation_key] = invitation_obj
            _increment_membership_version(settings)
            await _patch_settings(storage_config, space_id, space_meta, settings)
            msg = "Invitation token expired"
            raise RuntimeError(msg)

        role = invitation_obj.get("role")
        if not _is_mutable_member_role(role):
            msg = "Invitation has invalid role"
            raise TypeError(msg)

        now_iso = _now_iso()
        invitation_obj["state"] = "accepted"
        invitation_obj["accepted_at"] = now_iso
        invitation_obj["accepted_by"] = payload.accepted_by_user_id
        invitations[invitation_key] = invitation_obj

        members = settings["members"]
        members[payload.accepted_by_user_id] = _build_member_record(
            user_id=payload.accepted_by_user_id,
            role=cast("str", role),
            invited_by=cast("str", invitation_obj.get("invited_by", "")),
            invited_at=cast("str", invitation_obj.get("invited_at", now_iso)),
            state="active",
        )
        members[payload.accepted_by_user_id]["activated_at"] = now_iso

        _increment_membership_version(settings)
        await _patch_settings(storage_config, space_id, space_meta, settings)

    event = _audit_event(
        action="member.accept",
        actor_user_id=payload.accepted_by_user_id,
        space_id=space_id,
        target_user_id=payload.accepted_by_user_id,
        extra={"role": cast("str", role)},
    )
    return {
        "member": members[payload.accepted_by_user_id],
        "audit_event": event,
    }


async def update_member_role(
    storage_config: dict[str, str],
    space_id: str,
    payload: UpdateMemberRoleInput,
) -> dict[str, Any]:
    """Update role for an invited or active member."""
    if not _is_mutable_member_role(payload.role):
        msg = "role must be one of admin/editor/viewer"
        raise RuntimeError(msg)

    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)
        members = settings["members"]
        member_obj = members.get(payload.member_user_id)
        if member_obj is None:
            msg = f"Member not found: {payload.member_user_id}"
            raise RuntimeError(msg)
        if not isinstance(member_obj, dict):
            msg = f"Member record malformed: {payload.member_user_id}"
            raise TypeError(msg)
        if member_obj.get("state") == "revoked":
            msg = f"Member is revoked: {payload.member_user_id}"
            raise RuntimeError(msg)

        member_obj["role"] = payload.role
        member_obj["updated_at"] = _now_iso()
        members[payload.member_user_id] = member_obj
        _increment_membership_version(settings)
        await _patch_settings(storage_config, space_id, space_meta, settings)

    event = _audit_event(
        action="member.role_change",
        actor_user_id=payload.changed_by_user_id,
        space_id=space_id,
        target_user_id=payload.member_user_id,
        extra={"role": payload.role},
    )
    return {"member": member_obj, "audit_event": event}


async def revoke_member(
    storage_config: dict[str, str],
    space_id: str,
    payload: RevokeMemberInput,
) -> dict[str, Any]:
    """Revoke member access and invalidate pending invitations."""
    lock = await _space_lock(space_id)
    async with lock:
        space_meta_obj = await _core_any.get_space(storage_config, space_id)
        space_meta = cast("dict[str, Any]", space_meta_obj)
        settings = _normalize_settings(space_meta)

        owner_user_id = _owner_user_id(space_meta, settings)
        if owner_user_id and payload.member_user_id == owner_user_id:
            msg = "Owner cannot be revoked"
            raise RuntimeError(msg)

        members = settings["members"]
        member_obj = members.get(payload.member_user_id)
        if member_obj is None:
            msg = f"Member not found: {payload.member_user_id}"
            raise RuntimeError(msg)
        if not isinstance(member_obj, dict):
            msg = f"Member record malformed: {payload.member_user_id}"
            raise TypeError(msg)

        revoked_at = _now_iso()
        member_obj["state"] = "revoked"
        member_obj["revoked_at"] = revoked_at
        members[payload.member_user_id] = member_obj

        invitations = settings["invitations"]
        for token, invitation in invitations.items():
            if not isinstance(invitation, dict):
                continue
            same_user = invitation.get("user_id") == payload.member_user_id
            is_pending = invitation.get("state") == "pending"
            if same_user and is_pending:
                invitation["state"] = "revoked"
                invitation["revoked_at"] = revoked_at
                invitation["revoked_by"] = payload.revoked_by_user_id
                invitations[token] = invitation

        _increment_membership_version(settings)
        await _patch_settings(storage_config, space_id, space_meta, settings)

    event = _audit_event(
        action="member.revoke",
        actor_user_id=payload.revoked_by_user_id,
        space_id=space_id,
        target_user_id=payload.member_user_id,
    )
    return {"member": member_obj, "audit_event": event}


def is_active_member(space_meta: dict[str, Any], user_id: str) -> bool:
    """Return True when the user is an active member by lifecycle state."""
    settings = _normalize_settings(space_meta)
    members = settings.get("members")
    if not isinstance(members, dict):
        return False
    member_obj = members.get(user_id)
    return isinstance(member_obj, dict) and member_obj.get("state") == "active"


__all__ = [
    "AcceptInvitationInput",
    "InvitationDeliveryProvider",
    "InviteMemberInput",
    "MemberRole",
    "MemberState",
    "RevokeMemberInput",
    "TokenOnlyInvitationProvider",
    "UpdateMemberRoleInput",
    "accept_invitation",
    "create_invitation",
    "is_active_member",
    "list_members",
    "revoke_member",
    "update_member_role",
]
