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
