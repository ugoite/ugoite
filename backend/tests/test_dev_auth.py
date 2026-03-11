"""Local development login endpoint tests.

REQ-OPS-015: Local dev auth mode selection.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import time
from typing import TYPE_CHECKING

import pytest
from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache
from app.main import app

if TYPE_CHECKING:
    from pathlib import Path


def _totp_code(secret: str, timestamp: int) -> str:
    decoded_secret = base64.b32decode(secret.upper(), casefold=True)
    counter = timestamp // 30
    digest = hmac.new(
        decoded_secret,
        counter.to_bytes(8, "big"),
        hashlib.sha1,
    ).digest()
    offset = digest[-1] & 0x0F
    binary = int.from_bytes(digest[offset : offset + 4], "big") & 0x7FFFFFFF
    return f"{binary % 1_000_000:06d}"


def _configure_dev_auth_env(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
    *,
    mode: str,
    user_id: str = "dev-alice",
    signing_secret: str = "dev-signing-secret",
    signing_kid: str = "dev-local-v1",
) -> None:
    monkeypatch.setenv("UGOITE_ROOT", str(temp_space_root))
    monkeypatch.setenv("UGOITE_DEV_AUTH_MODE", mode)
    monkeypatch.setenv("UGOITE_DEV_USER_ID", user_id)
    monkeypatch.setenv("UGOITE_DEV_SIGNING_SECRET", signing_secret)
    monkeypatch.setenv("UGOITE_DEV_SIGNING_KID", signing_kid)
    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_SECRETS",
        f"{signing_kid}:{signing_secret}",
    )
    monkeypatch.setenv("UGOITE_AUTH_BEARER_ACTIVE_KIDS", signing_kid)


def test_dev_auth_req_ops_015_config_exposes_manual_totp_mode(
    monkeypatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: local browser/CLI login can discover manual-totp mode."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    clear_auth_manager_cache()

    client = TestClient(app)
    response = client.get("/auth/dev/config")

    assert response.status_code == 200
    assert response.json() == {
        "mode": "manual-totp",
        "username_hint": "dev-alice",
        "supports_manual_totp": True,
        "supports_mock_oauth": False,
    }


def test_dev_auth_req_ops_015_manual_totp_login_issues_signed_token(
    monkeypatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual-totp login issues a bearer token for protected APIs."""
    timestamp = int(time.time())
    secret = "JBSWY3DPEHPK3PXP"
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", "3600")
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    client = TestClient(app)
    login_response = client.post(
        "/auth/dev/login",
        json={
            "username": "dev-alice",
            "totp_code": _totp_code(secret, timestamp),
        },
    )

    assert login_response.status_code == 200
    token = login_response.json()["bearer_token"]
    protected_response = client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert protected_response.status_code == 200
    assert isinstance(protected_response.json(), list)


def test_dev_auth_req_ops_015_mock_oauth_login_issues_signed_token(
    monkeypatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: mock-oauth login remains explicit and returns a signed token."""
    timestamp = int(time.time())
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
        user_id="dev-oauth-user",
        signing_secret="oauth-signing-secret",
    )
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", "3600")
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    client = TestClient(app)
    login_response = client.post("/auth/dev/mock-oauth")

    assert login_response.status_code == 200
    token = login_response.json()["bearer_token"]
    protected_response = client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert protected_response.status_code == 200
    assert isinstance(protected_response.json(), list)


def test_dev_auth_req_ops_015_config_rejects_unsupported_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: local dev auth config rejects unsupported auth modes."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="automatic",
    )
    clear_auth_manager_cache()

    response = TestClient(app).get("/auth/dev/config")

    assert response.status_code == 503


def test_dev_auth_req_ops_015_config_requires_non_empty_user_id(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: config rejects blank local dev usernames."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
        user_id="   ",
    )
    clear_auth_manager_cache()

    response = TestClient(app).get("/auth/dev/config")

    assert response.status_code == 503


def test_dev_auth_req_ops_015_manual_login_rejects_non_manual_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login endpoint rejects sessions running in mock-oauth mode."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "dev-alice", "totp_code": "123456"},
    )

    assert response.status_code == 409
    assert "manual-totp login is not enabled" in response.json()["detail"]


def test_dev_auth_req_ops_015_manual_login_rejects_wrong_username(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login rejects usernames that do not match the terminal context."""
    timestamp = int(time.time())
    secret = "JBSWY3DPEHPK3PXP"
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "other-user", "totp_code": _totp_code(secret, timestamp)},
    )

    assert response.status_code == 401
    assert response.json()["detail"] == "Invalid username or 2FA code."


def test_dev_auth_req_ops_015_manual_login_requires_totp_secret(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login requires a configured local 2FA secret."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.delenv("UGOITE_DEV_2FA_SECRET", raising=False)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "dev-alice", "totp_code": "123456"},
    )

    assert response.status_code == 503


def test_dev_auth_req_ops_015_manual_login_rejects_invalid_totp_code(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login rejects invalid 2FA codes."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", "JBSWY3DPEHPK3PXP")
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "dev-alice", "totp_code": "000000"},
    )

    assert response.status_code == 401
    assert response.json()["detail"] == "Invalid username or 2FA code."


def test_dev_auth_req_ops_015_manual_login_requires_signing_material(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login cannot issue a token without signing material."""
    timestamp = int(time.time())
    secret = "JBSWY3DPEHPK3PXP"
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_SIGNING_SECRET", " ")
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", " ")
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
    )

    assert response.status_code == 503


@pytest.mark.parametrize(
    ("ttl_value", "expected_detail"),
    [
        ("abc", "UGOITE_DEV_AUTH_TTL_SECONDS must be an integer."),
        ("0", "UGOITE_DEV_AUTH_TTL_SECONDS must be positive."),
    ],
)
def test_dev_auth_req_ops_015_manual_login_validates_ttl(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
    ttl_value: str,
    expected_detail: str,
) -> None:
    """REQ-OPS-015: manual login validates dev auth TTL configuration."""
    timestamp = int(time.time())
    secret = "JBSWY3DPEHPK3PXP"
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", ttl_value)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/dev/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
    )

    assert response.status_code == 503


def test_dev_auth_req_ops_015_mock_oauth_rejects_manual_totp_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: mock-oauth endpoint rejects sessions running in manual-totp mode."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="manual-totp",
    )
    clear_auth_manager_cache()

    response = TestClient(app).post("/auth/dev/mock-oauth")

    assert response.status_code == 409
    assert "mock-oauth login is not enabled" in response.json()["detail"]
