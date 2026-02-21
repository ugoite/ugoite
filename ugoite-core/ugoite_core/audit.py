"""Audit logging primitives for security-relevant space events."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from typing import Any, cast

from . import _ugoite_core as _core

_DEFAULT_AUDIT_LIMIT = 100
_DEFAULT_AUDIT_RETENTION = 5000
_MAX_AUDIT_RETENTION = 50000
_core_any = cast("Any", _core)


@dataclass(frozen=True)
class AuditEventInput:
    """Input payload for audit event persistence."""

    action: str
    actor_user_id: str
    outcome: str
    target_type: str | None = None
    target_id: str | None = None
    request_method: str | None = None
    request_path: str | None = None
    request_id: str | None = None
    metadata: dict[str, Any] | None = None


@dataclass(frozen=True)
class AuditListFilter:
    """Filter and pagination options for audit retrieval."""

    offset: int = 0
    limit: int = _DEFAULT_AUDIT_LIMIT
    action: str | None = None
    actor_user_id: str | None = None
    outcome: str | None = None


def _retention_limit() -> int:
    raw = os.environ.get("UGOITE_AUDIT_RETENTION_MAX_EVENTS")
    if not isinstance(raw, str) or not raw.strip():
        return _DEFAULT_AUDIT_RETENTION
    try:
        parsed = int(raw)
    except ValueError:
        return _DEFAULT_AUDIT_RETENTION
    bounded = max(100, parsed)
    return min(_MAX_AUDIT_RETENTION, bounded)


def _normalize_outcome(outcome: str) -> str:
    value = outcome.strip().lower()
    return value if value in {"success", "deny", "error"} else "success"


async def append_audit_event(
    storage_config: dict[str, str],
    space_id: str,
    payload: AuditEventInput,
) -> dict[str, Any]:
    """Append a tamper-evident audit event to the space's JSONL audit log file."""
    action = payload.action.strip()
    if not action:
        msg = "audit action must not be empty"
        raise RuntimeError(msg)

    actor_user_id = payload.actor_user_id.strip()
    if not actor_user_id:
        msg = "actor_user_id must not be empty"
        raise RuntimeError(msg)
    event_payload = {
        "action": action,
        "actor_user_id": actor_user_id,
        "outcome": _normalize_outcome(payload.outcome),
        "target_type": payload.target_type,
        "target_id": payload.target_id,
        "request_method": payload.request_method,
        "request_path": payload.request_path,
        "request_id": payload.request_id,
        "metadata": payload.metadata or {},
    }
    return await _core_any.append_audit_event_py(
        storage_config,
        space_id,
        json.dumps(event_payload, separators=(",", ":"), sort_keys=True),
        _retention_limit(),
    )


async def list_audit_events(
    storage_config: dict[str, str],
    space_id: str,
    filters: AuditListFilter | None = None,
) -> dict[str, Any]:
    """List audit events with optional filters and pagination."""
    options = filters or AuditListFilter()
    return await _core_any.list_audit_events_py(
        storage_config,
        space_id,
        json.dumps(
            {
                "offset": max(0, options.offset),
                "limit": max(1, options.limit),
                "action": options.action,
                "actor_user_id": options.actor_user_id,
                "outcome": options.outcome,
            },
            separators=(",", ":"),
            sort_keys=True,
        ),
    )
