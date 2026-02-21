"""Service account tests.

REQ-SEC-009: Service Accounts and Scoped API Keys for Automation.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

import ugoite_core

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.asyncio
async def test_service_account_scopes_are_enforced(tmp_path: Path) -> None:
    """REQ-SEC-009: service API keys are limited to declared scopes."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "scope-space")

    await ugoite_core.create_service_account(
        config,
        "scope-space",
        ugoite_core.CreateServiceAccountInput(
            display_name="CI Bot",
            scopes=["entry_read"],
            created_by_user_id="owner",
        ),
    )
    accounts = await ugoite_core.list_service_accounts(config, "scope-space")
    service_account_id = str(accounts[0]["id"])

    key_result = await ugoite_core.create_service_account_key(
        config,
        "scope-space",
        ugoite_core.CreateServiceAccountKeyInput(
            service_account_id=service_account_id,
            key_name="read-only",
            created_by_user_id="owner",
        ),
    )

    identity = await ugoite_core.authenticate_headers_for_space(
        config,
        "scope-space",
        {"X-API-Key": str(key_result["secret"])},
    )
    await ugoite_core.require_space_action(
        config,
        "scope-space",
        identity,
        "entry_read",
    )

    with pytest.raises(ugoite_core.AuthorizationError) as exc_info:
        await ugoite_core.require_space_action(
            config,
            "scope-space",
            identity,
            "entry_write",
        )
    assert "missing required scope" in exc_info.value.detail


@pytest.mark.asyncio
async def test_service_account_revoked_key_is_rejected(tmp_path: Path) -> None:
    """REQ-SEC-009: revoked service API keys are rejected immediately."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "revoke-space")

    created = await ugoite_core.create_service_account(
        config,
        "revoke-space",
        ugoite_core.CreateServiceAccountInput(
            display_name="Deploy Bot",
            scopes=["entry_read", "entry_write"],
            created_by_user_id="owner",
        ),
    )
    account_id = str(created["id"])

    key_result = await ugoite_core.create_service_account_key(
        config,
        "revoke-space",
        ugoite_core.CreateServiceAccountKeyInput(
            service_account_id=account_id,
            key_name="deploy",
            created_by_user_id="owner",
        ),
    )
    key_id = str(key_result["key"]["id"])

    await ugoite_core.revoke_service_account_key(
        config,
        "revoke-space",
        ugoite_core.RevokeServiceAccountKeyInput(
            service_account_id=account_id,
            key_id=key_id,
            revoked_by_user_id="owner",
        ),
    )

    with pytest.raises(ugoite_core.AuthError, match="revoked"):
        await ugoite_core.authenticate_headers_for_space(
            config,
            "revoke-space",
            {"X-API-Key": str(key_result["secret"])},
        )


@pytest.mark.asyncio
async def test_service_account_key_usage_is_audit_logged(tmp_path: Path) -> None:
    """REQ-SEC-009: service key usage emits audit events for forensics."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    await ugoite_core.create_space(config, "audit-space")

    created = await ugoite_core.create_service_account(
        config,
        "audit-space",
        ugoite_core.CreateServiceAccountInput(
            display_name="Indexer Bot",
            scopes=["entry_read"],
            created_by_user_id="owner",
        ),
    )
    key_result = await ugoite_core.create_service_account_key(
        config,
        "audit-space",
        ugoite_core.CreateServiceAccountKeyInput(
            service_account_id=str(created["id"]),
            key_name="indexer",
            created_by_user_id="owner",
        ),
    )

    await ugoite_core.authenticate_headers_for_space(
        config,
        "audit-space",
        {"X-API-Key": str(key_result["secret"])},
        request_method="GET",
        request_path="/spaces/audit-space/entries",
        request_id="req-1",
    )

    events = await ugoite_core.list_audit_events(
        config,
        "audit-space",
        ugoite_core.AuditListFilter(action="service_account.key.use", limit=20),
    )
    assert events["total"] >= 1
    latest = events["items"][0]
    assert latest["action"] == "service_account.key.use"
    assert latest["request_path"] == "/spaces/audit-space/entries"
