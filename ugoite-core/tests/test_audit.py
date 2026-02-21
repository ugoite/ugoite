"""Audit logging tests.

REQ-SEC-008: Security Audit Logging and Attribution.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

import ugoite_core

if TYPE_CHECKING:
    import pathlib


@pytest.mark.asyncio
async def test_audit_appends_and_lists_events(tmp_path: pathlib.Path) -> None:
    """REQ-SEC-008: append/list preserves event attribution metadata."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "audit-space")

    await ugoite_core.append_audit_event(
        config,
        "audit-space",
        ugoite_core.AuditEventInput(
            action="entry.create",
            actor_user_id="alice",
            outcome="success",
            target_type="entry",
            target_id="entry-1",
            request_method="POST",
            request_path="/spaces/audit-space/entries",
        ),
    )

    result = await ugoite_core.list_audit_events(
        config,
        "audit-space",
        ugoite_core.AuditListFilter(limit=50),
    )
    assert result["total"] == 1
    assert len(result["items"]) == 1
    event = result["items"][0]
    assert event["action"] == "entry.create"
    assert event["actor_user_id"] == "alice"
    assert event["target_id"] == "entry-1"


@pytest.mark.asyncio
async def test_audit_detects_chain_tampering(tmp_path: pathlib.Path) -> None:
    """REQ-SEC-008: tampered audit chain is detected on read."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "audit-space")

    await ugoite_core.append_audit_event(
        config,
        "audit-space",
        ugoite_core.AuditEventInput(
            action="entry.update",
            actor_user_id="alice",
            outcome="success",
            target_type="entry",
            target_id="entry-1",
        ),
    )

    audit_file = Path(root) / "spaces" / "audit-space" / "audit" / "events.jsonl"
    records = [
        line for line in audit_file.read_text(encoding="utf-8").splitlines() if line
    ]
    tampered = json.loads(records[0])
    tampered["action"] = "entry.delete"
    audit_file.write_text(json.dumps(tampered) + "\n", encoding="utf-8")

    with pytest.raises(RuntimeError, match="integrity"):
        await ugoite_core.list_audit_events(config, "audit-space")


@pytest.mark.asyncio
async def test_audit_retention_preserves_chain_integrity(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-008: retention trimming keeps a valid verifiable hash chain."""
    monkeypatch.setenv("UGOITE_AUDIT_RETENTION_MAX_EVENTS", "100")
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "audit-space")

    for index in range(101):
        await ugoite_core.append_audit_event(
            config,
            "audit-space",
            ugoite_core.AuditEventInput(
                action="entry.update",
                actor_user_id="alice",
                outcome="success",
                target_type="entry",
                target_id=f"entry-{index}",
            ),
        )

    result = await ugoite_core.list_audit_events(config, "audit-space")
    assert result["total"] == 100


@pytest.mark.asyncio
async def test_audit_rejects_invalid_space_id(tmp_path: pathlib.Path) -> None:
    """REQ-SEC-008: path traversal style space identifiers are rejected."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}

    with pytest.raises(RuntimeError, match="invalid space_id"):
        await ugoite_core.append_audit_event(
            config,
            "../escape",
            ugoite_core.AuditEventInput(
                action="entry.create",
                actor_user_id="alice",
                outcome="success",
            ),
        )
