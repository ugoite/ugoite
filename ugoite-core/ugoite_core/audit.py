"""Audit logging primitives for security-relevant space events."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import secrets
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

_DEFAULT_AUDIT_LIMIT = 100
_MAX_AUDIT_LIMIT = 500
_DEFAULT_AUDIT_RETENTION = 5000
_space_locks: dict[str, asyncio.Lock] = {}
_space_locks_guard = asyncio.Lock()


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


def _now_iso() -> str:
    return datetime.now(tz=UTC).isoformat().replace("+00:00", "Z")


def _retention_limit() -> int:
    raw = os.environ.get("UGOITE_AUDIT_RETENTION_MAX_EVENTS")
    if not isinstance(raw, str) or not raw.strip():
        return _DEFAULT_AUDIT_RETENTION
    try:
        parsed = int(raw)
    except ValueError:
        return _DEFAULT_AUDIT_RETENTION
    return max(100, parsed)


def _event_hash(payload: dict[str, Any], prev_hash: str) -> str:
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    material = f"{prev_hash}:{canonical}".encode()
    return hashlib.sha256(material).hexdigest()


def _verify_chain(events: list[dict[str, Any]]) -> None:
    prev_hash = "root"
    for event in events:
        candidate = dict(event)
        expected_hash = candidate.pop("event_hash", None)
        candidate_prev_hash = candidate.get("prev_hash", "root")
        if not isinstance(expected_hash, str):
            msg = "Audit event missing event_hash"
            raise TypeError(msg)
        if candidate_prev_hash != prev_hash:
            msg = "Audit chain prev_hash mismatch"
            raise RuntimeError(msg)
        actual_hash = _event_hash(candidate, prev_hash)
        if actual_hash != expected_hash:
            msg = "Audit chain integrity check failed"
            raise RuntimeError(msg)
        prev_hash = expected_hash


def _normalize_outcome(outcome: str) -> str:
    value = outcome.strip().lower()
    return value if value in {"success", "deny", "error"} else "success"


async def _space_lock(space_id: str) -> asyncio.Lock:
    async with _space_locks_guard:
        existing = _space_locks.get(space_id)
        if existing is not None:
            return existing
        created = asyncio.Lock()
        _space_locks[space_id] = created
        return created


def _audit_file_path(storage_config: dict[str, str], space_id: str) -> Path:
    uri = storage_config.get("uri", "")
    if not uri.startswith("fs://"):
        msg = "audit logging currently supports fs:// storage only"
        raise RuntimeError(msg)
    root = uri.removeprefix("fs://")
    base = Path(root)
    return base / "spaces" / space_id / "audit" / "events.jsonl"


async def _read_events(
    storage_config: dict[str, str],
    space_id: str,
) -> list[dict[str, Any]]:
    path = _audit_file_path(storage_config, space_id)
    if not path.exists():
        return []
    events: list[dict[str, Any]] = []
    text = await asyncio.to_thread(path.read_text, "utf-8")
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            parsed = json.loads(stripped)
        except json.JSONDecodeError as exc:
            msg = "Audit log contains malformed JSON"
            raise RuntimeError(msg) from exc
        if isinstance(parsed, dict):
            events.append(parsed)
    return events


async def _write_events(
    storage_config: dict[str, str],
    space_id: str,
    events: list[dict[str, Any]],
) -> None:
    path = _audit_file_path(storage_config, space_id)
    await asyncio.to_thread(path.parent.mkdir, parents=True, exist_ok=True)
    lines = [json.dumps(item, separators=(",", ":"), sort_keys=True) for item in events]
    payload = "\n".join(lines)
    if payload:
        payload += "\n"
    tmp_path = path.with_suffix(".tmp")
    await asyncio.to_thread(tmp_path.write_text, payload, "utf-8")
    await asyncio.to_thread(tmp_path.replace, path)


async def append_audit_event(
    storage_config: dict[str, str],
    space_id: str,
    payload: AuditEventInput,
) -> dict[str, Any]:
    """Append a tamper-evident audit event to space settings metadata."""
    action = payload.action.strip()
    if not action:
        msg = "audit action must not be empty"
        raise RuntimeError(msg)

    actor_user_id = payload.actor_user_id.strip()
    if not actor_user_id:
        msg = "actor_user_id must not be empty"
        raise RuntimeError(msg)

    lock = await _space_lock(space_id)
    async with lock:
        events = await _read_events(storage_config, space_id)
        _verify_chain(events)

        prev_hash = "root"
        if events:
            prev_hash_obj = events[-1].get("event_hash")
            if isinstance(prev_hash_obj, str) and prev_hash_obj:
                prev_hash = prev_hash_obj

        event: dict[str, Any] = {
            "id": secrets.token_urlsafe(12),
            "timestamp": _now_iso(),
            "space_id": space_id,
            "action": action,
            "actor_user_id": actor_user_id,
            "outcome": _normalize_outcome(payload.outcome),
            "target_type": payload.target_type,
            "target_id": payload.target_id,
            "request_method": payload.request_method,
            "request_path": payload.request_path,
            "request_id": payload.request_id,
            "metadata": payload.metadata or {},
            "prev_hash": prev_hash,
        }

        event["event_hash"] = _event_hash(event, prev_hash)
        events.append(event)

        retention = _retention_limit()
        if len(events) > retention:
            events = events[-retention:]

        await _write_events(storage_config, space_id, events)
        return event


async def list_audit_events(
    storage_config: dict[str, str],
    space_id: str,
    filters: AuditListFilter | None = None,
) -> dict[str, Any]:
    """List audit events with optional filters and pagination."""
    lock = await _space_lock(space_id)
    async with lock:
        all_events = await _read_events(storage_config, space_id)
        _verify_chain(all_events)

    options = filters or AuditListFilter()
    normalized_limit = max(1, min(options.limit, _MAX_AUDIT_LIMIT))
    normalized_offset = max(0, options.offset)
    normalized_action = (
        options.action.strip()
        if isinstance(options.action, str) and options.action.strip()
        else None
    )
    normalized_actor = (
        options.actor_user_id.strip()
        if isinstance(options.actor_user_id, str) and options.actor_user_id.strip()
        else None
    )
    normalized_outcome = (
        options.outcome.strip().lower()
        if isinstance(options.outcome, str) and options.outcome.strip()
        else None
    )

    filtered = all_events
    if normalized_action:
        filtered = [
            item for item in filtered if item.get("action") == normalized_action
        ]
    if normalized_actor:
        filtered = [
            item for item in filtered if item.get("actor_user_id") == normalized_actor
        ]
    if normalized_outcome:
        filtered = [
            item for item in filtered if item.get("outcome") == normalized_outcome
        ]

    filtered.sort(key=lambda item: str(item.get("timestamp", "")), reverse=True)
    total = len(filtered)
    page = filtered[normalized_offset : normalized_offset + normalized_limit]
    return {
        "items": page,
        "total": total,
        "offset": normalized_offset,
        "limit": normalized_limit,
    }
