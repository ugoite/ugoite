"""Tests to achieve 100% backend coverage.

Each test references at least one REQ-* requirement identifier.
These tests focus on error handling paths, edge cases, and untested
endpoints not covered by existing test suites.
"""

from __future__ import annotations

import asyncio
import json as _json
from collections.abc import AsyncGenerator
from pathlib import Path
from typing import TYPE_CHECKING, Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
import ugoite_core
from fastapi import HTTPException
from starlette.responses import Response, StreamingResponse

if TYPE_CHECKING:
    from fastapi.testclient import TestClient

from app.api.endpoints.forms import _persist_form_acl_settings
from app.api.endpoints.search import _is_sql_error
from app.api.endpoints.space import (
    _format_form_validation_errors,
    _sanitize_space_meta,
    _validate_entry_markdown_against_form,
)
from app.core.auth import require_authenticated_identity
from app.core.authorization import request_identity
from app.core.ids import validate_uuid
from app.core.middleware import (
    _AuditRequestEvent,
    _capture_response_body,
    _emit_audit_event,
    security_middleware,
)
from app.core.security import is_local_host, resolve_client_host
from app.core.storage import _ensure_local_root, storage_config_from_root
from app.mcp.server import _context_headers, list_entries

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _amock(**kwargs: Any) -> AsyncMock:
    """Return an AsyncMock configured with keyword arguments."""
    return AsyncMock(**kwargs)


# ===========================================================================
# 1. core/ids.py - validate_uuid (lines 22-27)
# ===========================================================================


def test_validate_uuid_valid() -> None:
    """REQ-SEC-009: validate_uuid accepts well-formed UUIDs."""
    val = "550e8400-e29b-41d4-a716-446655440000"
    assert validate_uuid(val, "some_id") == val


def test_validate_uuid_invalid_raises() -> None:
    """REQ-SEC-009: validate_uuid rejects malformed values."""
    with pytest.raises(ValueError, match="Invalid some_id"):
        validate_uuid("not-a-uuid", "some_id")


# ===========================================================================
# 2. core/auth.py - require_authenticated_identity (line 41)
# ===========================================================================


def test_require_authenticated_identity_missing_raises() -> None:
    """REQ-SEC-003: unauthenticated request raises 401."""
    request = MagicMock()
    request.state = MagicMock()
    request.state.identity = None

    with pytest.raises(HTTPException) as exc_info:
        require_authenticated_identity(request)
    assert exc_info.value.status_code == 401


# ===========================================================================
# 3. core/authorization.py - request_identity (line 16)
# ===========================================================================


def test_request_identity_missing_raises() -> None:
    """REQ-SEC-003: request_identity raises 401 when no identity is set."""
    request = MagicMock()
    request.state = MagicMock()
    request.state.identity = None  # no identity set (unauthenticated)

    with pytest.raises(HTTPException) as exc_info:
        request_identity(request)
    assert exc_info.value.status_code == 401


# ===========================================================================
# 4. core/security.py - resolve_client_host (line 54)
# ===========================================================================


def test_resolve_client_host_trusted_proxy() -> None:
    """REQ-SEC-001: resolve_client_host honors X-Forwarded-For with trust flag."""
    headers = {"x-forwarded-for": "203.0.113.5, 198.51.100.1"}
    result = resolve_client_host(headers, "10.0.0.1", trust_proxy_headers=True)
    assert result == "203.0.113.5"


def test_resolve_client_host_empty_forwarded() -> None:
    """REQ-SEC-001: resolve_client_host falls back when forwarded header is empty."""
    headers = {"x-forwarded-for": "   "}
    result = resolve_client_host(headers, "10.0.0.1", trust_proxy_headers=True)
    assert result == "10.0.0.1"


# ===========================================================================
# 5. core/storage.py - _ensure_local_root (lines 20-27, 31-33)
# ===========================================================================


def test_ensure_local_root_file_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root handles file:// URIs."""
    target = tmp_path / "new_dir"
    _ensure_local_root(f"file://{target}")
    assert target.exists()


def test_ensure_local_root_fs_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root handles fs:// URIs."""
    target = tmp_path / "fs_dir"
    _ensure_local_root(f"fs://{target}")
    assert target.exists()


def test_ensure_local_root_oserror_plain_path(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root propagates OSError for plain paths."""
    with (
        patch("pathlib.Path.mkdir", side_effect=OSError("Permission denied")),
        pytest.raises(OSError, match="Permission denied"),
    ):
        _ensure_local_root("/nonexistent/deeply/nested/path")


def test_ensure_local_root_oserror_file_scheme(tmp_path: Path) -> None:
    """REQ-STO-001: _ensure_local_root propagates OSError for file:// paths."""
    with (
        patch("pathlib.Path.mkdir", side_effect=OSError("Permission denied")),
        pytest.raises(OSError, match="Permission denied"),
    ):
        _ensure_local_root(f"file://{tmp_path}/blocked")


def test_ensure_local_root_non_file_scheme() -> None:
    """REQ-STO-001: _ensure_local_root returns early for non-file schemes."""
    # Should not raise - s3:// scheme is not file-based, just returns
    _ensure_local_root("s3://my-bucket/spaces")


# ===========================================================================
# 6. core/middleware.py - edge cases (lines 93-94, 177, 184-185, 229)
# ===========================================================================


def test_capture_response_body_without_iterator() -> None:
    """REQ-SEC-002: _capture_response_body reads body attribute when no iterator."""
    response = Response(content=b"direct body content")
    result = asyncio.run(_capture_response_body(response))
    assert result == b"direct body content"


def test_middleware_sse_response_not_captured(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-001: SSE responses bypass body capture in security middleware."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.client.host = "127.0.0.1"
    mock_request.url.path = "/"
    mock_request.headers = {}

    async def _gen() -> AsyncGenerator[bytes]:
        yield b"data: test\n\n"

    sse_response = StreamingResponse(_gen(), media_type="text/event-stream")

    async def _call_next(_req: object) -> Response:
        return sse_response

    result = asyncio.run(security_middleware(mock_request, _call_next))
    assert result is sse_response


def test_middleware_403_non_json_body_handled(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-002: middleware handles non-JSON 403 response body gracefully."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.client.host = "127.0.0.1"
    mock_request.url.path = "/"
    mock_request.method = "GET"
    mock_request.headers = {}

    forbidden_response = Response(content=b"not valid json", status_code=403)

    async def _call_next(_req: object) -> Response:
        return forbidden_response

    async def _fake_sign(_body: bytes, _root: object) -> tuple[str, str]:
        return "kid", "sig"

    with patch("app.core.middleware.build_response_signature", _fake_sign):
        result = asyncio.run(security_middleware(mock_request, _call_next))
    # Should not raise; 403 with non-JSON body is handled
    assert result.status_code == 403


def test_middleware_emit_audit_runtime_error_swallowed(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-008: RuntimeError in audit emission is logged and swallowed."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    mock_request = MagicMock()
    mock_request.url.path = "/spaces/test-space/entries"
    mock_request.headers = {}
    mock_request.method = "GET"

    event = _AuditRequestEvent(
        action="data.mutation",
        outcome="success",
        actor_user_id="user1",
    )

    with patch(
        "ugoite_core.append_audit_event",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        # Should not raise; RuntimeError is caught and logged
        asyncio.run(_emit_audit_event(mock_request, event))


# ===========================================================================
# 7. mcp/server.py - _context_headers and list_entries (lines 28-54, 60-83)
# ===========================================================================


def test_context_headers_request_none_raises() -> None:
    """REQ-API-001: _context_headers raises when request context is None."""
    ctx = MagicMock()
    ctx.request_context.request = None
    with pytest.raises(RuntimeError, match="Missing authentication context"):
        _context_headers(ctx)


def test_context_headers_headers_none_raises() -> None:
    """REQ-API-001: _context_headers raises when headers cannot be resolved."""
    ctx = MagicMock()
    # request has no headers attribute (enforced by the spec) and is not a dict
    request = MagicMock(spec=["method", "url", "path"])
    ctx.request_context.request = request
    with pytest.raises(RuntimeError, match="Missing request headers"):
        _context_headers(ctx)


def test_context_headers_dict_request() -> None:
    """REQ-API-001: _context_headers resolves headers from dict-style request."""
    ctx = MagicMock()
    ctx.request_context.request = {
        "headers": {"authorization": "Bearer token"},
        "method": "GET",
        "path": "/spaces/s/entries",
    }
    headers, _, _, _ = _context_headers(ctx)
    assert headers == {"authorization": "Bearer token"}


def test_context_headers_with_url_object() -> None:
    """REQ-API-001: _context_headers extracts path from request.url.path."""
    ctx = MagicMock()
    request = MagicMock()
    request.headers = {"authorization": "Bearer token", "x-request-id": "req-123"}
    request.url.path = "/spaces/test/entries"
    request.method = "GET"
    ctx.request_context.request = request
    _, _, path, req_id = _context_headers(ctx)
    assert path == "/spaces/test/entries"
    assert req_id == "req-123"


def test_list_entries_mcp(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-002: MCP list_entries resource returns JSON-encoded entries."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    ctx = MagicMock()
    request = MagicMock()
    request.headers = {"authorization": "Bearer test-token"}
    request.url.path = "/spaces/mcp-space/entries"
    request.method = "GET"
    ctx.request_context.request = request

    fake_identity = MagicMock()
    fake_entries = [{"id": "e1", "content": "# Hello"}]

    async def _run() -> str:
        with (
            patch(
                "app.mcp.server.authenticate_headers_for_space",
                _amock(return_value=fake_identity),
            ),
            patch(
                "ugoite_core.require_space_action",
                _amock(return_value=None),
            ),
            patch(
                "ugoite_core.list_entries",
                _amock(return_value=fake_entries),
            ),
            patch(
                "ugoite_core.filter_readable_entries",
                _amock(return_value=fake_entries),
            ),
        ):
            return await list_entries("mcp-space", ctx)

    result = asyncio.run(_run())
    assert _json.loads(result) == fake_entries


# ===========================================================================
# 8. asset.py - missing endpoints and error paths
# ===========================================================================


def test_list_assets_success(test_client: TestClient) -> None:
    """REQ-API-001: list assets returns empty list for new space."""
    test_client.post("/spaces", json={"name": "asset-list-ws"})
    response = test_client.get("/spaces/asset-list-ws/assets")
    assert response.status_code == 200
    assert response.json() == []


def test_list_assets_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list assets returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/asset-authz-ws/assets")
    assert response.status_code == 403


def test_list_assets_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: list assets returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "asset-err-ws"})
    with patch(
        "ugoite_core.list_assets",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.get("/spaces/asset-err-ws/assets")
    assert response.status_code == 500


def test_upload_asset_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: upload asset returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-upload-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_write",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/asset-upload-authz-ws/assets",
            files={"file": ("test.txt", b"hello", "text/plain")},
        )
    assert response.status_code == 403


def test_delete_asset_success(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 200 when asset is deleted successfully."""
    test_client.post("/spaces", json={"name": "asset-del-ws"})
    upload_response = test_client.post(
        "/spaces/asset-del-ws/assets",
        files={"file": ("test.txt", b"hello asset", "text/plain")},
    )
    assert upload_response.status_code == 201
    asset_id = upload_response.json()["id"]

    delete_response = test_client.delete(
        f"/spaces/asset-del-ws/assets/{asset_id}",
    )
    assert delete_response.status_code == 200
    assert delete_response.json()["status"] == "deleted"


def test_delete_asset_runtime_error_generic(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 500 for non-ref/non-notfound RuntimeError."""
    test_client.post("/spaces", json={"name": "asset-del-err-ws"})
    with patch(
        "ugoite_core.delete_asset",
        _amock(side_effect=RuntimeError("unexpected storage error")),
    ):
        response = test_client.delete(
            "/spaces/asset-del-err-ws/assets/some-asset",
        )
    assert response.status_code == 500


def test_delete_asset_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: delete asset returns 500 for unexpected exception."""
    test_client.post("/spaces", json={"name": "asset-del-exc-ws"})
    with patch(
        "ugoite_core.delete_asset",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.delete(
            "/spaces/asset-del-exc-ws/assets/some-asset",
        )
    assert response.status_code == 500


# ===========================================================================
# 9. audit.py - error paths (lines 61-76)
# ===========================================================================


def test_list_audit_events_integrity_error(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 409 on integrity chain error."""
    test_client.post("/spaces", json={"name": "audit-chain-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("integrity chain violation")),
    ):
        response = test_client.get("/spaces/audit-chain-ws/audit/events")
    assert response.status_code == 409


def test_list_audit_events_not_found(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "audit-notfound-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/audit-notfound-ws/audit/events")
    assert response.status_code == 404


def test_list_audit_events_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-008: audit events endpoint returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "audit-rterr-ws"})
    with patch(
        "ugoite_core.list_audit_events",
        _amock(side_effect=RuntimeError("unexpected storage error")),
    ):
        response = test_client.get("/spaces/audit-rterr-ws/audit/events")
    # "unexpected storage error" does not contain "integrity", "chain", or "not found"
    assert response.status_code == 500


# ===========================================================================
# 10. entry.py - error paths
# ===========================================================================


def test_create_entry_already_exists(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 409 when entry already exists."""
    test_client.post("/spaces", json={"name": "entry-dup-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("entry already exists")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-dup-ws/entries",
            json={"id": "e-dup-2", "content": "# Title\n"},
        )
    assert response.status_code == 409


def test_create_entry_form_error(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 422 when form name is unknown."""
    test_client.post("/spaces", json={"name": "entry-form-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("unknown form type")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-form-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 422


def test_create_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-exc-ws"})
    with (
        patch(
            "ugoite_core.require_markdown_write",
            _amock(return_value=None),
        ),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=ValueError("unexpected error")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-exc-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 500


def test_list_entries_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: list entries returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-list-exc-ws"})
    with patch(
        "ugoite_core.list_entries",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/entry-list-exc-ws/entries")
    assert response.status_code == 500


def test_get_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-get-404-ws"})
    response = test_client.get("/spaces/entry-get-404-ws/entries/missing-entry")
    assert response.status_code == 404


def test_get_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "entry-get-exc-ws"})
    with patch(
        "ugoite_core.get_entry",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/entry-get-exc-ws/entries/e1")
    assert response.status_code == 500


def test_update_entry_conflict_then_404(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 409 on revision conflict."""
    test_client.post("/spaces", json={"name": "entry-upd-conflict-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("revision conflict detected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-conflict-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "old-rev"},
        )
    assert response.status_code == 409


def test_update_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-upd-404-ws"})
    with patch(
        "ugoite_core.get_entry",
        _amock(side_effect=RuntimeError("entry not found")),
    ):
        response = test_client.put(
            "/spaces/entry-upd-404-ws/entries/missing",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 404


def test_update_entry_form_validation_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 422 for form validation failure."""
    test_client.post("/spaces", json={"name": "entry-upd-form-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("unknown form reference")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-form-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 422


def test_update_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-upd-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("generic storage error")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-rt-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 500


def test_update_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: update entry returns 500 on unexpected non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-upd-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-exc-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 500


def test_delete_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-del-404-ws"})
    response = test_client.delete("/spaces/entry-del-404-ws/entries/missing")
    assert response.status_code == 404


def test_delete_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-del-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.delete_entry",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-rt-ws/entries/e1")
    assert response.status_code == 500


def test_delete_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: delete entry returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-del-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.delete_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-exc-ws/entries/e1")
    assert response.status_code == 500


def test_get_entry_history_not_found(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-hist-404-ws"})
    response = test_client.get(
        "/spaces/entry-hist-404-ws/entries/missing/history",
    )
    assert response.status_code == 404


def test_get_entry_history_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-hist-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_history",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-rt-ws/entries/e1/history",
        )
    assert response.status_code == 500


def test_get_entry_history_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry history returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-hist-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_history",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-exc-ws/entries/e1/history",
        )
    assert response.status_code == 500


def test_get_entry_revision_not_found(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 404 when revision does not exist."""
    test_client.post("/spaces", json={"name": "entry-rev-404-ws"})
    test_client.post(
        "/spaces/entry-rev-404-ws/entries",
        json={"id": "e1", "content": "# Title\n"},
    )
    response = test_client.get(
        "/spaces/entry-rev-404-ws/entries/e1/history/missing-rev",
    )
    assert response.status_code == 404


def test_get_entry_revision_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-rev-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_revision",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-rt-ws/entries/e1/history/some-rev",
        )
    assert response.status_code == 500


def test_get_entry_revision_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: get entry revision returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-rev-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry_revision",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-exc-ws/entries/e1/history/some-rev",
        )
    assert response.status_code == 500


def test_restore_entry_not_found(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 404 when entry does not exist."""
    test_client.post("/spaces", json={"name": "entry-restore-404-ws"})
    response = test_client.post(
        "/spaces/entry-restore-404-ws/entries/missing/restore",
        json={"revision_id": "rev1"},
    )
    assert response.status_code == 404


def test_restore_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-restore-rt-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.restore_entry",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-rt-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 500


def test_restore_entry_generic_exception(test_client: TestClient) -> None:
    """REQ-API-002: restore entry returns 500 on non-runtime exception."""
    test_client.post("/spaces", json={"name": "entry-restore-exc-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch(
            "ugoite_core.restore_entry",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-exc-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 500


# ===========================================================================
# 11. forms.py - error paths (lines 45-51, 88, 96-97, 99-103, 127-133, 160-168, 243-249)
# ===========================================================================


def test_format_form_validation_errors_branches() -> None:
    """REQ-API-004: _format_form_validation_errors handles all warning shapes."""
    errors = [
        {"message": "Value is required"},
        {"field": "body_field"},
        {},
    ]
    result = _format_form_validation_errors(errors)
    assert "Value is required" in result
    assert "Invalid field: body_field" in result
    assert "Form validation error" in result


def test_persist_form_acl_settings_get_space_runtime_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-006: _persist_form_acl_settings recovers from RuntimeError.

    Handles RuntimeError from get_space gracefully.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    with (
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=RuntimeError("space not found")),
        ),
        patch(
            "ugoite_core.patch_space",
            _amock(return_value={}),
        ),
    ):
        asyncio.run(
            _persist_form_acl_settings(
                storage_config,
                "test-space",
                "Note",
                [{"type": "user", "user_id": "u1"}],
                None,
            ),
        )


def test_list_forms_skips_nameless_form(test_client: TestClient) -> None:
    """REQ-API-004: list forms skips forms without a valid name."""
    test_client.post("/spaces", json={"name": "forms-noname-ws"})
    fake_forms = [{"name": None}, {"name": "ValidForm", "fields": {}}]
    with (
        patch(
            "ugoite_core.list_forms",
            _amock(return_value=fake_forms),
        ),
        patch(
            "ugoite_core.require_form_read",
            _amock(return_value=None),
        ),
    ):
        response = test_client.get("/spaces/forms-noname-ws/forms")
    assert response.status_code == 200
    result = response.json()
    assert all(f.get("name") for f in result)


def test_list_forms_skips_unauthorized_form(test_client: TestClient) -> None:
    """REQ-SEC-006: list forms skips forms for which access is denied."""
    test_client.post("/spaces", json={"name": "forms-authz-ws"})
    fake_forms = [{"name": "SecretForm"}, {"name": "PublicForm"}]
    call_count = {"n": 0}

    async def _require_form_read_side_effect(*args: object, **kwargs: object) -> None:
        call_count["n"] += 1
        if call_count["n"] == 1:
            err_code = "forbidden"
            raise ugoite_core.AuthorizationError(err_code, "no access", "form_read")

    with (
        patch(
            "ugoite_core.list_forms",
            _amock(return_value=fake_forms),
        ),
        patch(
            "ugoite_core.require_form_read",
            AsyncMock(side_effect=_require_form_read_side_effect),
        ),
    ):
        response = test_client.get("/spaces/forms-authz-ws/forms")
    assert response.status_code == 200
    # Only the non-denied form should appear
    assert len(response.json()) == 1


def test_list_forms_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: list forms returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "forms-err-ws"})
    with patch(
        "ugoite_core.list_forms",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/forms-err-ws/forms")
    assert response.status_code == 500


def test_list_form_types_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: list form types returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "formtypes-err-ws"})
    with patch(
        "ugoite_core.list_column_types",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/formtypes-err-ws/forms/types")
    assert response.status_code == 500


def test_get_form_not_found(test_client: TestClient) -> None:
    """REQ-API-004: get form returns 404 when form does not exist."""
    test_client.post("/spaces", json={"name": "form-get-404-ws"})
    response = test_client.get("/spaces/form-get-404-ws/forms/MissingForm")
    assert response.status_code == 404


def test_get_form_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-004: get form returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "form-get-rt-ws"})
    with (
        patch("ugoite_core.require_form_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_form",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get("/spaces/form-get-rt-ws/forms/SomeForm")
    assert response.status_code == 500


def test_create_form_generic_exception(test_client: TestClient) -> None:
    """REQ-API-004: create form returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "form-create-exc-ws"})
    with patch(
        "ugoite_core.upsert_form",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/form-create-exc-ws/forms",
            json={
                "name": "TestForm",
                "version": 1,
                "template": "# TestForm\n",
                "fields": {"Body": {"type": "markdown"}},
            },
        )
    assert response.status_code == 500


# ===========================================================================
# 12. members.py - missing endpoints and error paths
# ===========================================================================


def test_list_members_success(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns the owner as a member."""
    test_client.post("/spaces", json={"name": "members-list-ws"})
    response = test_client.get("/spaces/members-list-ws/members")
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_list_members_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/members-authz-ws/members")
    assert response.status_code == 403


def test_list_members_not_found_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 404 when space not found error occurs."""
    test_client.post("/spaces", json={"name": "members-notfound-ws"})
    with patch(
        "ugoite_core.list_members",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/members-notfound-ws/members")
    assert response.status_code == 404


def test_list_members_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: list members returns 500 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-rt-ws"})
    with patch(
        "ugoite_core.list_members",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.get("/spaces/members-rt-ws/members")
    assert response.status_code == 500


def test_invite_member_already_active(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 409 when member is already active."""
    test_client.post("/spaces", json={"name": "members-dup-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("member already active")),
    ):
        response = test_client.post(
            "/spaces/members-dup-ws/members/invitations",
            json={"user_id": "alice", "role": "viewer"},
        )
    assert response.status_code == 409


def test_invite_member_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 404 when space/user not found."""
    test_client.post("/spaces", json={"name": "members-inv-404-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("user not found")),
    ):
        response = test_client.post(
            "/spaces/members-inv-404-ws/members/invitations",
            json={"user_id": "unknown-user", "role": "viewer"},
        )
    assert response.status_code == 404


def test_invite_member_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 400 on other runtime error."""
    test_client.post("/spaces", json={"name": "members-inv-rt-ws"})
    with patch(
        "ugoite_core.create_invitation",
        _amock(side_effect=RuntimeError("validation failed")),
    ):
        response = test_client.post(
            "/spaces/members-inv-rt-ws/members/invitations",
            json={"user_id": "alice-x", "role": "viewer"},
        )
    assert response.status_code == 400


def test_accept_invitation_expired(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 410 when invitation is expired."""
    test_client.post("/spaces", json={"name": "members-expired-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("invitation expired")),
    ):
        response = test_client.post(
            "/spaces/members-expired-ws/members/accept",
            json={"token": "expired-token"},
        )
    assert response.status_code == 410


def test_accept_invitation_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 404 when token not found."""
    test_client.post("/spaces", json={"name": "members-accept-404-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("invitation not found")),
    ):
        response = test_client.post(
            "/spaces/members-accept-404-ws/members/accept",
            json={"token": "bad-token"},
        )
    assert response.status_code == 404


def test_accept_invitation_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: accept invitation returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-accept-rt-ws"})
    with patch(
        "ugoite_core.accept_invitation",
        _amock(side_effect=RuntimeError("bad request")),
    ):
        response = test_client.post(
            "/spaces/members-accept-rt-ws/members/accept",
            json={"token": "bad-token"},
        )
    assert response.status_code == 400


def test_update_member_role_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 404 when member does not exist."""
    test_client.post("/spaces", json={"name": "members-role-404-ws"})
    with patch(
        "ugoite_core.update_member_role",
        _amock(side_effect=RuntimeError("member not found")),
    ):
        response = test_client.post(
            "/spaces/members-role-404-ws/members/absent-user/role",
            json={"role": "editor"},
        )
    assert response.status_code == 404


def test_update_member_role_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-role-rt-ws"})
    with patch(
        "ugoite_core.update_member_role",
        _amock(side_effect=RuntimeError("invalid operation")),
    ):
        response = test_client.post(
            "/spaces/members-role-rt-ws/members/alice/role",
            json={"role": "editor"},
        )
    assert response.status_code == 400


def test_revoke_member_not_found(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 404 when member does not exist."""
    test_client.post("/spaces", json={"name": "members-revoke-404-ws"})
    with patch(
        "ugoite_core.revoke_member",
        _amock(side_effect=RuntimeError("member not found")),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-404-ws/members/absent-user",
        )
    assert response.status_code == 404


def test_revoke_member_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "members-revoke-rt-ws"})
    with patch(
        "ugoite_core.revoke_member",
        _amock(side_effect=RuntimeError("already revoked")),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-rt-ws/members/alice",
        )
    assert response.status_code == 400


# ===========================================================================
# 13. search.py - error paths (lines 27, 47, 73-83, 121)
# ===========================================================================


def test_is_sql_error_function() -> None:
    """REQ-SRCH-003: _is_sql_error detects SQL error prefix."""
    assert _is_sql_error("UGOITE_SQL_ERROR: invalid syntax") is True
    assert _is_sql_error("  UGOITE_SQL_ERROR: x") is True
    assert _is_sql_error("some other error") is False


def test_query_endpoint_sql_filter_rejected(test_client: TestClient) -> None:
    """REQ-SRCH-003: query endpoint rejects SQL filter keys."""
    test_client.post("/spaces", json={"name": "query-sql-ws"})
    response = test_client.post(
        "/spaces/query-sql-ws/query",
        json={"filter": {"sql": "SELECT 1"}},
    )
    assert response.status_code == 400


def test_query_endpoint_sql_error_returns_400(test_client: TestClient) -> None:
    """REQ-SRCH-003: query endpoint returns 400 for SQL-type errors."""
    test_client.post("/spaces", json={"name": "query-sqlerr-ws"})
    with patch(
        "ugoite_core.query_index",
        _amock(side_effect=RuntimeError("UGOITE_SQL_ERROR: bad syntax")),
    ):
        response = test_client.post(
            "/spaces/query-sqlerr-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 400


def test_query_endpoint_generic_exception(test_client: TestClient) -> None:
    """REQ-SRCH-001: query endpoint returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "query-exc-ws"})
    with patch(
        "ugoite_core.query_index",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.post(
            "/spaces/query-exc-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 500


def test_search_endpoint_generic_exception(test_client: TestClient) -> None:
    """REQ-SRCH-001: search endpoint returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "search-exc-ws"})
    with patch(
        "ugoite_core.search_entries",
        _amock(side_effect=RuntimeError("index failure")),
    ):
        response = test_client.get("/spaces/search-exc-ws/search?q=test")
    assert response.status_code == 500


# ===========================================================================
# 14. service_accounts.py - missing endpoints and error paths
# ===========================================================================


def test_list_service_accounts_success(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns empty list for new space."""
    test_client.post("/spaces", json={"name": "sa-list-ws"})
    response = test_client.get("/spaces/sa-list-ws/service-accounts")
    assert response.status_code == 200
    assert isinstance(response.json(), list)


def test_list_service_accounts_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.get("/spaces/sa-authz-ws/service-accounts")
    assert response.status_code == 403


def test_list_service_accounts_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "sa-notfound-ws"})
    with patch(
        "ugoite_core.list_service_accounts",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.get("/spaces/sa-notfound-ws/service-accounts")
    assert response.status_code == 404


def test_list_service_accounts_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-009: list service accounts returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-rt-ws"})
    with patch(
        "ugoite_core.list_service_accounts",
        _amock(side_effect=RuntimeError("validation failed")),
    ):
        response = test_client.get("/spaces/sa-rt-ws/service-accounts")
    assert response.status_code == 400


def test_create_service_account_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 404 when space not found."""
    test_client.post("/spaces", json={"name": "sa-create-404-ws"})
    with patch(
        "ugoite_core.create_service_account",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.post(
            "/spaces/sa-create-404-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 404


def test_create_service_account_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-create-rt-ws"})
    with patch(
        "ugoite_core.create_service_account",
        _amock(side_effect=RuntimeError("invalid scope")),
    ):
        response = test_client.post(
            "/spaces/sa-create-rt-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 400


def test_create_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account key returns 404 when SA not found."""
    test_client.post("/spaces", json={"name": "sa-key-404-ws"})
    create_response = test_client.post(
        "/spaces/sa-key-404-ws/service-accounts",
        json={"display_name": "Bot", "scopes": ["entry_read"]},
    )
    sa_id = create_response.json()["id"]
    with patch(
        "ugoite_core.create_service_account_key",
        _amock(side_effect=RuntimeError("service account not found")),
    ):
        response = test_client.post(
            f"/spaces/sa-key-404-ws/service-accounts/{sa_id}/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 404


def test_create_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: create service account key returns 400 on generic runtime error."""
    test_client.post("/spaces", json={"name": "sa-key-rt-ws"})
    create_response = test_client.post(
        "/spaces/sa-key-rt-ws/service-accounts",
        json={"display_name": "Bot", "scopes": ["entry_read"]},
    )
    sa_id = create_response.json()["id"]
    with patch(
        "ugoite_core.create_service_account_key",
        _amock(side_effect=RuntimeError("key limit exceeded")),
    ):
        response = test_client.post(
            f"/spaces/sa-key-rt-ws/service-accounts/{sa_id}/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 400


def test_rotate_service_account_key_full_flow(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns a new secret."""
    test_client.post("/spaces", json={"name": "sa-rotate-ws"})
    create_sa = test_client.post(
        "/spaces/sa-rotate-ws/service-accounts",
        json={"display_name": "RotateBot", "scopes": ["entry_read"]},
    )
    sa_id = create_sa.json()["id"]
    create_key = test_client.post(
        f"/spaces/sa-rotate-ws/service-accounts/{sa_id}/keys",
        json={"key_name": "original-key"},
    )
    key_id = create_key.json()["key"]["id"]

    response = test_client.post(
        f"/spaces/sa-rotate-ws/service-accounts/{sa_id}/keys/{key_id}/rotate",
        json={"key_name": "rotated-key"},
    )
    assert response.status_code == 201


def test_rotate_service_account_key_authz_error(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-rotate-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sa-rotate-authz-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 403


def test_rotate_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: rotate service account key returns 404 when key not found."""
    test_client.post("/spaces", json={"name": "sa-rotate-404-ws"})
    with patch(
        "ugoite_core.rotate_service_account_key",
        _amock(side_effect=RuntimeError("key not found")),
    ):
        response = test_client.post(
            "/spaces/sa-rotate-404-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 404


def test_rotate_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: rotate service account key returns 400 on generic error."""
    test_client.post("/spaces", json={"name": "sa-rotate-rt-ws"})
    with patch(
        "ugoite_core.rotate_service_account_key",
        _amock(side_effect=RuntimeError("invalid operation")),
    ):
        response = test_client.post(
            "/spaces/sa-rotate-rt-ws/service-accounts/sa-id/keys/key-id/rotate",
            json={"key_name": "rotated"},
        )
    assert response.status_code == 400


def test_revoke_service_account_key_not_found(test_client: TestClient) -> None:
    """REQ-SEC-009: revoke service account key returns 404 when key not found."""
    test_client.post("/spaces", json={"name": "sa-revoke-404-ws"})
    with patch(
        "ugoite_core.revoke_service_account_key",
        _amock(side_effect=RuntimeError("key not found")),
    ):
        response = test_client.delete(
            "/spaces/sa-revoke-404-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 404


def test_revoke_service_account_key_generic_runtime_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: revoke service account key returns 400 on generic error."""
    test_client.post("/spaces", json={"name": "sa-revoke-rt-ws"})
    with patch(
        "ugoite_core.revoke_service_account_key",
        _amock(side_effect=RuntimeError("already revoked")),
    ):
        response = test_client.delete(
            "/spaces/sa-revoke-rt-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 400


# ===========================================================================
# 15. space.py - error paths
# ===========================================================================


def test_sanitize_space_meta_without_settings() -> None:
    """REQ-API-001: _sanitize_space_meta returns early when settings is not a dict."""
    result = _sanitize_space_meta({"id": "test", "name": "test", "settings": "bad"})
    assert result["id"] == "test"
    assert result["settings"] == "bad"


def test_ensure_space_exists_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: _ensure_space_exists returns 500 for non-notfound RuntimeError."""
    test_client.post("/spaces", json={"name": "space-ens-rt-ws"})
    with patch(
        "ugoite_core.get_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.get("/spaces/space-ens-rt-ws/entries")
    assert response.status_code == 500


def test_list_spaces_exception(test_client: TestClient) -> None:
    """REQ-API-001: list spaces returns 500 on unexpected exception."""
    with patch(
        "ugoite_core.list_spaces",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 500


def test_list_spaces_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: list spaces returns 500 on RuntimeError."""
    with patch(
        "ugoite_core.list_spaces",
        _amock(side_effect=RuntimeError("storage failure")),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 500


def test_list_spaces_skips_unauthorized_space(test_client: TestClient) -> None:
    """REQ-API-001: list spaces skips spaces the user cannot access."""
    test_client.post("/spaces", json={"name": "visible-ws"})
    test_client.post("/spaces", json={"name": "hidden-ws"})
    call_count = {"n": 0}

    async def _require_side_effect(*args: object, **kwargs: object) -> None:
        call_count["n"] += 1
        if call_count["n"] == 2:
            err_code = "forbidden"
            raise ugoite_core.AuthorizationError(err_code, "no access", "space_list")

    with patch(
        "ugoite_core.require_space_action",
        AsyncMock(side_effect=_require_side_effect),
    ):
        response = test_client.get("/spaces")
    assert response.status_code == 200
    assert len(response.json()) == 1


def test_create_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: create space returns 500 for generic RuntimeError."""
    with patch(
        "ugoite_core.create_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post("/spaces", json={"name": "fail-ws"})
    assert response.status_code == 500


def test_create_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: create space returns 500 for non-runtime exception."""
    with patch(
        "ugoite_core.create_space",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post("/spaces", json={"name": "fail-exc-ws"})
    assert response.status_code == 500


def test_get_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: get space returns 500 for generic RuntimeError."""
    test_client.post("/spaces", json={"name": "space-get-rt-ws"})
    with (
        patch("ugoite_core.require_space_action", _amock(return_value=None)),
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=RuntimeError("storage error")),
        ),
    ):
        response = test_client.get("/spaces/space-get-rt-ws")
    assert response.status_code == 500


def test_get_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: get space returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "space-get-exc-ws"})
    with (
        patch("ugoite_core.require_space_action", _amock(return_value=None)),
        patch(
            "ugoite_core.get_space",
            _amock(side_effect=ValueError("unexpected")),
        ),
    ):
        response = test_client.get("/spaces/space-get-exc-ws")
    assert response.status_code == 500


def test_patch_space_with_name(test_client: TestClient) -> None:
    """REQ-API-001: patch space can update name field."""
    test_client.post("/spaces", json={"name": "patchable-ws"})
    response = test_client.patch(
        "/spaces/patchable-ws",
        json={"name": "updated-name"},
    )
    assert response.status_code == 200


def test_patch_space_not_found_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 404 for not-found RuntimeError."""
    test_client.post("/spaces", json={"name": "patch-404-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=RuntimeError("space not found")),
    ):
        response = test_client.patch(
            "/spaces/patch-404-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 404


def test_patch_space_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 500 for generic RuntimeError."""
    test_client.post("/spaces", json={"name": "patch-rt-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.patch(
            "/spaces/patch-rt-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 500


def test_patch_space_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: patch space returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "patch-exc-ws"})
    with patch(
        "ugoite_core.patch_space",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.patch(
            "/spaces/patch-exc-ws",
            json={"settings": {"key": "value"}},
        )
    assert response.status_code == 500


def test_test_connection_value_error(test_client: TestClient) -> None:
    """REQ-STO-006: test-connection returns 400 when storage config is invalid."""
    test_client.post("/spaces", json={"name": "conn-test-ws"})
    with patch(
        "ugoite_core.test_storage_connection",
        _amock(side_effect=ValueError("invalid storage config")),
    ):
        response = test_client.post(
            "/spaces/conn-test-ws/test-connection",
            json={"storage_config": {"uri": "invalid://bad"}},
        )
    assert response.status_code == 400


def test_validate_entry_form_not_found_non_error_runtime(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-004: _validate_entry_markdown_against_form returns 500.

    Applies to non-notfound form errors.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    # Use lowercase '## form' so extract_properties returns key 'form'
    with (
        patch(
            "ugoite_core.get_form",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
        pytest.raises(HTTPException) as exc_info,
    ):
        asyncio.run(
            _validate_entry_markdown_against_form(
                storage_config,
                "test-space",
                "# Note\n\n## form\nNote\n",
            ),
        )
    assert exc_info.value.status_code == 500


def test_validate_entry_form_validation_warnings(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-004: _validate_entry_markdown_against_form raises 422.

    Raised when form validation has warnings.
    """
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    fake_form = {"name": "Note", "fields": {"Body": {"type": "markdown"}}}
    fake_warnings = [{"message": "required field missing"}]
    # Use lowercase '## form' so extract_properties returns key 'form'
    with (
        patch("ugoite_core.get_form", _amock(return_value=fake_form)),
        patch(
            "ugoite_core.validate_properties",
            return_value=({}, fake_warnings),
        ),
        pytest.raises(HTTPException) as exc_info,
    ):
        asyncio.run(
            _validate_entry_markdown_against_form(
                storage_config,
                "test-space",
                "# Note\n\n## form\nNote\n",
            ),
        )
    assert exc_info.value.status_code == 422


# ===========================================================================
# 16. sql.py - error paths
# ===========================================================================


def test_list_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: list SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/sql-authz-ws/sql")
    assert response.status_code == 403


def test_list_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: list SQL returns 500 on unexpected error."""
    test_client.post("/spaces", json={"name": "sql-list-err-ws"})
    with patch(
        "ugoite_core.list_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/sql-list-err-ws/sql")
    assert response.status_code == 500


def test_create_sql_already_exists(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 409 when entry already exists."""
    test_client.post("/spaces", json={"name": "sql-dup-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=RuntimeError("sql entry already exists")),
    ):
        response = test_client.post(
            "/spaces/sql-dup-ws/sql",
            json={"name": "Dup", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 409


def test_create_sql_validation_error(test_client: TestClient) -> None:
    """REQ-API-007: create SQL returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sql-val-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(
            side_effect=RuntimeError("UGOITE_SQL_VALIDATION: reserved keyword"),
        ),
    ):
        response = test_client.post(
            "/spaces/sql-val-ws/sql",
            json={"name": "Bad", "sql": "SELECT SYSTEM", "variables": []},
        )
    assert response.status_code == 422


def test_create_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-create-rt-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/sql-create-rt-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 500


def test_create_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-create-exc-ws"})
    with patch(
        "ugoite_core.create_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/sql-create-exc-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 500


def test_get_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-get-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/sql-get-authz-ws/sql/some-id")
    assert response.status_code == 403


def test_get_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-get-exc-ws"})
    with patch(
        "ugoite_core.get_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get("/spaces/sql-get-exc-ws/sql/some-id")
    assert response.status_code == 500


def test_update_sql_conflict(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 409 on revision conflict."""
    test_client.post("/spaces", json={"name": "sql-upd-conflict-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("conflict detected")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-conflict-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "old-rev",
            },
        )
    assert response.status_code == 409


def test_update_sql_not_found(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 404 when entry not found."""
    test_client.post("/spaces", json={"name": "sql-upd-404-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("entry not found")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-404-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 404


def test_update_sql_validation_error(test_client: TestClient) -> None:
    """REQ-API-007: update SQL returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sql-upd-val-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("UGOITE_SQL_VALIDATION: bad syntax")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-val-ws/sql/some-sql-id",
            json={
                "name": "Bad",
                "sql": "INVALID SQL",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 422


def test_update_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-upd-rt-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-rt-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 500


def test_update_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-upd-exc-ws"})
    with patch(
        "ugoite_core.update_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.put(
            "/spaces/sql-upd-exc-ws/sql/some-sql-id",
            json={
                "name": "Updated",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 500


def test_delete_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-del-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.delete("/spaces/sql-del-authz-ws/sql/some-sql-id")
    assert response.status_code == 403


def test_delete_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-del-rt-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.delete("/spaces/sql-del-rt-ws/sql/some-sql-id")
    assert response.status_code == 500


def test_delete_sql_generic_exception(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sql-del-exc-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.delete("/spaces/sql-del-exc-ws/sql/some-sql-id")
    assert response.status_code == 500


# ===========================================================================
# 17. sql_sessions.py - error paths
# ===========================================================================


def test_create_sql_session_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sess-authz-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 403


def test_create_sql_session_validation_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 422 for validation error."""
    test_client.post("/spaces", json={"name": "sess-val-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(
            side_effect=RuntimeError(
                "UGOITE_SQL_VALIDATION: missing placeholder",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sess-val-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 422


def test_create_sql_session_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sess-rt-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/sess-rt-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 500


def test_create_sql_session_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: create SQL session returns 500 for non-runtime exception."""
    test_client.post("/spaces", json={"name": "sess-exc-ws"})
    with patch(
        "ugoite_core.create_sql_session",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/sess-exc-ws/sql-sessions",
            json={"sql": "SELECT 1"},
        )
    assert response.status_code == 500


def test_get_sql_session_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-get-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-get-authz-ws/sql-sessions/some-session",
        )
    assert response.status_code == 403


def test_get_sql_session_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-get-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_status",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-get-exc-ws/sql-sessions/some-session",
        )
    assert response.status_code == 500


def test_get_sql_session_count_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session count returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-cnt-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-cnt-authz-ws/sql-sessions/some-session/count",
        )
    assert response.status_code == 403


def test_get_sql_session_count_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session count returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-cnt-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_count",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-cnt-exc-ws/sql-sessions/some-session/count",
        )
    assert response.status_code == 500


def test_get_sql_session_rows_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session rows returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-rows-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-rows-authz-ws/sql-sessions/some-session/rows",
        )
    assert response.status_code == 403


def test_get_sql_session_rows_generic_exception(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session rows returns 500 for unexpected error."""
    test_client.post("/spaces", json={"name": "sess-rows-exc-ws"})
    with patch(
        "ugoite_core.get_sql_session_rows",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.get(
            "/spaces/sess-rows-exc-ws/sql-sessions/some-session/rows",
        )
    assert response.status_code == 500


def test_get_sql_session_stream_authorization_error(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sess-stream-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_read",
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/sess-stream-authz-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 403


def test_get_sql_session_stream_success(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns NDJSON rows."""
    test_client.post("/spaces", json={"name": "sess-stream-ws"})

    page1 = {"rows": [{"id": "r1"}]}
    page2 = {"rows": []}

    call_count = {"n": 0}

    async def _rows_side(*args: object, **kwargs: object) -> dict[str, object]:
        call_count["n"] += 1
        return page1 if call_count["n"] == 1 else page2

    with patch("ugoite_core.get_sql_session_rows", AsyncMock(side_effect=_rows_side)):
        response = test_client.get(
            "/spaces/sess-stream-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert "r1" in response.text


# ===========================================================================
# Additional tests to reach 100% coverage
# ===========================================================================


# --- core/security.py line 54: is_local_host(None) ---


def test_is_local_host_none_returns_true() -> None:
    """REQ-SEC-001: is_local_host returns True when host is None."""
    assert is_local_host(None) is True


# --- core/storage.py line 22: empty path in file:// URI ---


def test_ensure_local_root_file_scheme_empty_path() -> None:
    """REQ-STO-001: _ensure_local_root raises ValueError for empty local path."""
    # Mock Path so str() returns "" to trigger the empty path guard
    mock_path_instance = MagicMock()
    mock_path_instance.__str__ = MagicMock(return_value="")

    with (
        patch("app.core.storage.Path", return_value=mock_path_instance),
        pytest.raises(ValueError, match="Local storage path is empty"),
    ):
        _ensure_local_root("file:///some/path")


# --- asset.py lines 55-57: generic Exception in upload_asset ---


def test_upload_asset_generic_exception(test_client: TestClient) -> None:
    """REQ-API-001: upload asset returns 500 on unexpected exception."""
    test_client.post("/spaces", json={"name": "asset-upload-exc-ws"})
    with patch(
        "ugoite_core.save_asset",
        _amock(side_effect=ValueError("unexpected")),
    ):
        response = test_client.post(
            "/spaces/asset-upload-exc-ws/assets",
            files={"file": ("test.txt", b"hello", "text/plain")},
        )
    assert response.status_code == 500


# --- asset.py line 114: AuthorizationError in delete_asset ---


def test_delete_asset_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: delete asset returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "asset-del-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "asset_write",
            ),
        ),
    ):
        response = test_client.delete(
            "/spaces/asset-del-authz-ws/assets/some-asset",
        )
    assert response.status_code == 403


# --- audit.py line 62: AuthorizationError in list_audit_events ---


def test_list_audit_events_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-008: list audit events returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "audit-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.get("/spaces/audit-authz-ws/audit/events")
    assert response.status_code == 403


# --- entry.py line 79: generic RuntimeError 500 in create_entry ---


def test_create_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: create entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-create-rt-ws"})
    with (
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.create_entry",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-create-rt-ws/entries",
            json={"content": "# Title\n"},
        )
    assert response.status_code == 500


# --- entry.py line 152: generic RuntimeError 500 in get_entry ---


def test_get_entry_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: get entry returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "entry-get-rt-ws"})
    with (
        patch("ugoite_core.require_entry_read", _amock(return_value=None)),
        patch(
            "ugoite_core.get_entry",
            _amock(side_effect=RuntimeError("storage corruption")),
        ),
    ):
        response = test_client.get("/spaces/entry-get-rt-ws/entries/e1")
    assert response.status_code == 500


# --- entry.py line 217: AuthorizationError in update_entry ---


def test_update_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: update entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-upd-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-authz-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "rev"},
        )
    assert response.status_code == 403


# --- entry.py line 235: inner except RuntimeError in conflict retry ---


def test_update_entry_conflict_retry_runtime_error(test_client: TestClient) -> None:
    """REQ-API-002: update entry 409 when conflict retry also fails."""
    test_client.post("/spaces", json={"name": "entry-upd-retry-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    # First get_entry returns fake_entry, second raises RuntimeError
    with (
        patch(
            "ugoite_core.get_entry",
            AsyncMock(
                side_effect=[
                    fake_entry,
                    RuntimeError("storage error during retry"),
                ],
            ),
        ),
        patch("ugoite_core.require_entry_write", _amock(return_value=None)),
        patch("ugoite_core.require_markdown_write", _amock(return_value=None)),
        patch(
            "ugoite_core.update_entry",
            _amock(side_effect=RuntimeError("revision conflict detected")),
        ),
    ):
        response = test_client.put(
            "/spaces/entry-upd-retry-ws/entries/e1",
            json={"markdown": "# Updated\n", "parent_revision_id": "old-rev"},
        )
    assert response.status_code == 409


# --- entry.py line 286: AuthorizationError in delete_entry ---


def test_delete_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: delete entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-del-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.delete("/spaces/entry-del-authz-ws/entries/e1")
    assert response.status_code == 403


# --- entry.py line 330: AuthorizationError in get_entry_history ---


def test_get_entry_history_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get entry history returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-hist-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_read",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_read",
                ),
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-hist-authz-ws/entries/e1/history",
        )
    assert response.status_code == 403


# --- entry.py line 379: AuthorizationError in get_entry_revision ---


def test_get_entry_revision_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get entry revision returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-rev-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_read",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_read",
                ),
            ),
        ),
    ):
        response = test_client.get(
            "/spaces/entry-rev-authz-ws/entries/e1/history/rev1",
        )
    assert response.status_code == 403


# --- entry.py line 427: AuthorizationError in restore_entry ---


def test_restore_entry_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: restore entry returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "entry-restore-authz-ws"})
    fake_entry = {"id": "e1", "content": "# Title\n", "revision_id": "rev1"}
    with (
        patch("ugoite_core.get_entry", _amock(return_value=fake_entry)),
        patch(
            "ugoite_core.require_entry_write",
            _amock(
                side_effect=ugoite_core.AuthorizationError(
                    "forbidden",
                    "no access",
                    "entry_write",
                ),
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/entry-restore-authz-ws/entries/e1/restore",
            json={"revision_id": "rev1"},
        )
    assert response.status_code == 403


# --- forms.py line 45: dict comprehension for existing form ACLs ---


def test_persist_form_acl_with_existing_acls(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-006: _persist_form_acl_settings reads existing form ACLs."""
    monkeypatch.setenv("UGOITE_ROOT", str(tmp_path))

    storage_config = storage_config_from_root(tmp_path)
    space_meta = {
        "settings": {
            "form_acls": {
                "OtherForm": {"read": [], "write": []},
            },
        },
    }
    with (
        patch("ugoite_core.get_space", _amock(return_value=space_meta)),
        patch("ugoite_core.patch_space", _amock(return_value={})),
    ):
        asyncio.run(
            _persist_form_acl_settings(
                storage_config,
                "test-space",
                "Note",
                [{"type": "user", "user_id": "u1"}],
                None,
            ),
        )


# --- forms.py line 100: AuthorizationError in list_forms ---


def test_list_forms_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list forms returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "forms-outer-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/forms-outer-authz-ws/forms")
    assert response.status_code == 403


# --- forms.py lines 128, 131: AuthorizationError and HTTPException re-raise ---


def test_list_form_types_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: list form types returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "formtypes-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/formtypes-authz-ws/forms/types")
    assert response.status_code == 403


def test_list_form_types_reraises_http_exception(test_client: TestClient) -> None:
    """REQ-API-004: list form types re-raises HTTPException from column types call."""
    test_client.post("/spaces", json={"name": "formtypes-httpexc-ws"})
    with patch(
        "ugoite_core.list_column_types",
        _amock(side_effect=HTTPException(status_code=503, detail="unavailable")),
    ):
        response = test_client.get("/spaces/formtypes-httpexc-ws/forms/types")
    assert response.status_code == 503


# --- forms.py line 161: AuthorizationError in get_form ---


def test_get_form_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: get form returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "form-authz-ws"})
    with patch(
        "ugoite_core.require_form_read",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "form_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/form-authz-ws/forms/SecretForm")
    assert response.status_code == 403


# --- forms.py line 243: generic RuntimeError 500 in create_form ---


def test_create_form_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-004: create form returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "form-create-rt-ws"})
    with patch(
        "ugoite_core.upsert_form",
        _amock(side_effect=RuntimeError("storage error")),
    ):
        response = test_client.post(
            "/spaces/form-create-rt-ws/forms",
            json={
                "name": "TestForm",
                "version": 1,
                "template": "# TestForm\n",
                "fields": {"Body": {"type": "markdown"}},
            },
        )
    assert response.status_code == 500


# --- members.py line 91: AuthorizationError in invite_member ---


def test_invite_member_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: invite member returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-inv-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/members-inv-authz-ws/members/invitations",
            json={"user_id": "alice", "role": "viewer"},
        )
    assert response.status_code == 403


# --- members.py line 182: AuthorizationError in update_member_role ---


def test_update_member_role_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: update member role returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-role-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/members-role-authz-ws/members/alice/role",
            json={"role": "editor"},
        )
    assert response.status_code == 403


# --- members.py line 226: AuthorizationError in revoke_member ---


def test_revoke_member_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-007: revoke member returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "members-revoke-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.delete(
            "/spaces/members-revoke-authz-ws/members/alice",
        )
    assert response.status_code == 403


# --- search.py line 74: AuthorizationError in query_endpoint ---


def test_query_endpoint_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: query endpoint returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "query-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "entry_read",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/query-authz-ws/query",
            json={"filter": {}},
        )
    assert response.status_code == 403


# --- search.py line 121: AuthorizationError in search_endpoint ---


def test_search_endpoint_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-006: search endpoint returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "search-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "entry_read",
            ),
        ),
    ):
        response = test_client.get("/spaces/search-authz-ws/search?q=test")
    assert response.status_code == 403


# --- service_accounts.py line 85: AuthorizationError in create_service_account ---


def test_create_service_account_authorization_error(test_client: TestClient) -> None:
    """REQ-SEC-009: create service account returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-create-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sa-create-authz-ws/service-accounts",
            json={"display_name": "Bot", "scopes": ["entry_read"]},
        )
    assert response.status_code == 403


# --- service_accounts.py line 133: AuthorizationError in create_service_account_key ---


def test_create_service_account_key_authorization_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: create service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-key-create-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sa-key-create-authz-ws/service-accounts/sa-id/keys",
            json={"key_name": "ci-key"},
        )
    assert response.status_code == 403


# --- service_accounts.py line 230: AuthorizationError in revoke_service_account_key ---


def test_revoke_service_account_key_authorization_error(
    test_client: TestClient,
) -> None:
    """REQ-SEC-009: revoke service account key returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sa-revoke-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.delete(
            "/spaces/sa-revoke-authz-ws/service-accounts/sa-id/keys/key-id",
        )
    assert response.status_code == 403


# --- space.py line 365: AuthorizationError in test_connection ---


def test_test_connection_authorization_error(test_client: TestClient) -> None:
    """REQ-STO-006: test-connection returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "conn-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "space_admin",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/conn-authz-ws/test-connection",
            json={"storage_config": {"uri": "s3://bucket"}},
        )
    assert response.status_code == 403


# --- sql.py line 90: AuthorizationError in create_sql ---


def test_create_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: create SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-create-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.post(
            "/spaces/sql-create-authz-ws/sql",
            json={"name": "Test", "sql": "SELECT 1", "variables": []},
        )
    assert response.status_code == 403


# --- sql.py line 145: generic RuntimeError 500 in get_sql ---


def test_get_sql_generic_runtime_error(test_client: TestClient) -> None:
    """REQ-API-006: get SQL returns 500 for generic runtime error."""
    test_client.post("/spaces", json={"name": "sql-get-rt-ws"})
    with patch(
        "ugoite_core.get_sql",
        _amock(side_effect=RuntimeError("storage corruption")),
    ):
        response = test_client.get("/spaces/sql-get-rt-ws/sql/some-id")
    assert response.status_code == 500


# --- sql.py line 194: AuthorizationError in update_sql ---


def test_update_sql_authorization_error(test_client: TestClient) -> None:
    """REQ-API-006: update SQL returns 403 on authorization failure."""
    test_client.post("/spaces", json={"name": "sql-upd-authz-ws"})
    with patch(
        "ugoite_core.require_space_action",
        _amock(
            side_effect=ugoite_core.AuthorizationError(
                "forbidden",
                "no access",
                "sql_write",
            ),
        ),
    ):
        response = test_client.put(
            "/spaces/sql-upd-authz-ws/sql/some-id",
            json={
                "name": "Test",
                "sql": "SELECT 1",
                "variables": [],
                "parent_revision_id": "rev",
            },
        )
    assert response.status_code == 403


# --- sql.py line 249: delete_sql not found RuntimeError ---


def test_delete_sql_not_found(test_client: TestClient) -> None:
    """REQ-API-006: delete SQL returns 404 when SQL entry not found."""
    test_client.post("/spaces", json={"name": "sql-del-404-ws"})
    with patch(
        "ugoite_core.delete_sql",
        _amock(side_effect=RuntimeError("sql entry not found")),
    ):
        response = test_client.delete("/spaces/sql-del-404-ws/sql/some-id")
    assert response.status_code == 404


# --- sql_sessions.py line 223: empty rows in stream generator ---


def test_get_sql_session_stream_empty_rows(test_client: TestClient) -> None:
    """REQ-API-008: get SQL session stream returns empty body when no rows."""
    test_client.post("/spaces", json={"name": "sess-stream-empty-ws"})
    with patch(
        "ugoite_core.get_sql_session_rows",
        _amock(return_value={"rows": []}),
    ):
        response = test_client.get(
            "/spaces/sess-stream-empty-ws/sql-sessions/some-session/stream",
        )
    assert response.status_code == 200
    assert response.text == ""
