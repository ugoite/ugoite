"""Regression tests for API error sanitization."""

import logging

import pytest
import ugoite_core
from fastapi.testclient import TestClient


def test_server_error_detail_is_sanitized(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-001: 500 HTTP details must be sanitized into stable public schema."""
    test_client.post("/spaces", json={"name": "test-ws"})

    async def _raise(*_args: object, **_kwargs: object) -> list[dict[str, object]]:
        msg = "leak /workspace/backend/private-path"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "search_entries", _raise)

    response = test_client.get("/spaces/test-ws/search", params={"q": "hello"})
    assert response.status_code == 500
    assert response.json()["detail"] == {
        "code": "internal_error",
        "message": "Internal server error",
    }


def test_server_error_detail_with_failed_prefix_is_sanitized_and_logged(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
    caplog: pytest.LogCaptureFixture,
) -> None:
    """REQ-API-001: raw 500 detail stays in server logs.

    Operator-facing text must not leak back to clients.
    """
    test_client.post("/spaces", json={"name": "members-ws"})

    async def _raise(*_args: object, **_kwargs: object) -> list[dict[str, object]]:
        msg = "Failed to list members for /workspace/backend/private-path"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "list_members", _raise)
    caplog.set_level(logging.ERROR, logger="app.main")

    response = test_client.get("/spaces/members-ws/members")

    assert response.status_code == 500
    assert response.json()["detail"] == {
        "code": "internal_error",
        "message": "Internal server error",
    }
    assert "Failed to list members for /workspace/backend/private-path" in caplog.text
