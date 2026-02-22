"""Regression tests for API error sanitization."""

import pytest
import ugoite_core
from fastapi.testclient import TestClient


def test_server_error_detail_is_sanitized(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-001: 5xx HTTP details must be sanitized into stable public schema."""
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
