"""Planned authentication enforcement tests.

REQ-SEC-003: Mandatory User Authentication.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import time
from typing import TYPE_CHECKING

import pytest
from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache
from app.main import app

if TYPE_CHECKING:
    from pathlib import Path


def _signed_token(payload: dict[str, object], secret: str) -> str:
    payload_segment = (
        base64.urlsafe_b64encode(
            json.dumps(payload, separators=(",", ":")).encode("utf-8"),
        )
        .decode("utf-8")
        .rstrip("=")
    )
    signature = hmac.new(
        secret.encode("utf-8"),
        payload_segment.encode("utf-8"),
        hashlib.sha256,
    ).digest()
    signature_segment = base64.urlsafe_b64encode(signature).decode("utf-8").rstrip("=")
    return f"v1.{payload_segment}.{signature_segment}"


@pytest.fixture
def unauthenticated_client(temp_space_root: Path) -> TestClient:
    """Return a TestClient without auth headers."""
    return TestClient(app)


def test_auth_rejects_unauthenticated_localhost_requests(
    unauthenticated_client: TestClient,
) -> None:
    """REQ-SEC-003: localhost requests require authenticated user identity."""
    response = unauthenticated_client.get("/spaces")
    assert response.status_code == 401
    assert response.json()["code"] == "missing_credentials"


def test_auth_rejects_unauthenticated_remote_requests(
    unauthenticated_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: remote requests require authenticated user identity."""
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    response = unauthenticated_client.get(
        "/spaces",
        headers={"x-forwarded-for": "203.0.113.20"},
    )
    assert response.status_code == 401
    assert response.json()["code"] == "missing_credentials"


def test_auth_rejects_invalid_bearer_signature(
    unauthenticated_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: invalid bearer signatures are rejected."""
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", "kid-1:correct-secret")
    clear_auth_manager_cache()
    bad = _signed_token(
        {
            "kid": "kid-1",
            "sub": "user-1",
            "exp": int(time.time()) + 3600,
        },
        secret="wrong-secret",
    )
    response = unauthenticated_client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {bad}"},
    )
    assert response.status_code == 401
    assert response.json()["code"] == "invalid_signature"


def test_auth_rejects_malformed_signed_token_segments(
    unauthenticated_client: TestClient,
) -> None:
    """REQ-SEC-003: malformed signed-token base64 segments are rejected."""
    malformed = "v1.@@@.%%%"
    response = unauthenticated_client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {malformed}"},
    )
    assert response.status_code == 401
    assert response.json()["code"] == "invalid_signature"


def test_auth_rejects_expired_bearer_token(
    unauthenticated_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: expired bearer tokens are rejected."""
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", "kid-1:test-secret")
    clear_auth_manager_cache()
    expired = _signed_token(
        {
            "kid": "kid-1",
            "sub": "user-1",
            "exp": int(time.time()) - 1,
        },
        secret="test-secret",
    )
    response = unauthenticated_client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {expired}"},
    )
    assert response.status_code == 401
    assert response.json()["code"] == "expired_token"


def test_auth_rejects_revoked_api_key(
    unauthenticated_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: revoked API keys are rejected."""
    monkeypatch.setenv(
        "UGOITE_AUTH_API_KEYS_JSON",
        json.dumps(
            {
                "api-key-1": {
                    "user_id": "svc-worker",
                    "principal_type": "service",
                    "key_id": "svc-key-1",
                },
            },
        ),
    )
    monkeypatch.setenv("UGOITE_AUTH_REVOKED_KEY_IDS", "svc-key-1")
    clear_auth_manager_cache()
    response = unauthenticated_client.get(
        "/spaces",
        headers={"X-API-Key": "api-key-1"},
    )
    assert response.status_code == 401
    assert response.json()["code"] == "revoked_key"


from unittest.mock import MagicMock

from fastapi import HTTPException

from app.core.auth import require_authenticated_identity
from app.core.authorization import request_identity
from app.core.ids import validate_uuid
from app.core.security import is_local_host, resolve_client_host


def test_validate_uuid_valid() -> None:
    """REQ-SEC-009: validate_uuid accepts well-formed UUIDs."""
    val = "550e8400-e29b-41d4-a716-446655440000"
    assert validate_uuid(val, "some_id") == val


def test_validate_uuid_invalid_raises() -> None:
    """REQ-SEC-009: validate_uuid rejects malformed values."""
    with pytest.raises(ValueError, match="Invalid some_id"):
        validate_uuid("not-a-uuid", "some_id")


def test_require_authenticated_identity_missing_raises() -> None:
    """REQ-SEC-003: unauthenticated request raises 401."""
    request = MagicMock()
    request.state = MagicMock()
    request.state.identity = None

    with pytest.raises(HTTPException) as exc_info:
        require_authenticated_identity(request)
    assert exc_info.value.status_code == 401


def test_request_identity_missing_raises() -> None:
    """REQ-SEC-003: request_identity raises 401 when no identity is set."""
    request = MagicMock()
    request.state = MagicMock()
    request.state.identity = None  # no identity set (unauthenticated)

    with pytest.raises(HTTPException) as exc_info:
        request_identity(request)
    assert exc_info.value.status_code == 401


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


def test_is_local_host_none_returns_true() -> None:
    """REQ-SEC-001: is_local_host returns True when host is None."""
    assert is_local_host(None) is True
