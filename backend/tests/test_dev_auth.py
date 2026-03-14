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

DEFAULT_TEST_USER_ID = "dev-alice"
DEFAULT_TEST_SIGNING_KID = "{}-{}-{}".format("dev", "local", "v1")
DEFAULT_TEST_SIGNING_SECRET = "{}-{}-{}".format("dev", "signing", "secret")
TEST_TOTP_SECRET = base64.b32encode(b"local-dev-auth-secret").decode("ascii")


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
    overrides: dict[str, str] | None = None,
) -> None:
    env = {
        "UGOITE_DEV_AUTH_MODE": mode,
        "UGOITE_DEV_USER_ID": DEFAULT_TEST_USER_ID,
        "UGOITE_DEV_SIGNING_SECRET": DEFAULT_TEST_SIGNING_SECRET,
        "UGOITE_DEV_SIGNING_KID": DEFAULT_TEST_SIGNING_KID,
    }
    if overrides:
        env.update(overrides)

    monkeypatch.setenv("UGOITE_ROOT", str(temp_space_root))
    for key, value in env.items():
        monkeypatch.setenv(key, value)
    monkeypatch.setenv(
        "UGOITE_AUTH_BEARER_SECRETS",
        f"{env['UGOITE_DEV_SIGNING_KID']}:{env['UGOITE_DEV_SIGNING_SECRET']}",
    )
    monkeypatch.setenv("UGOITE_AUTH_BEARER_ACTIVE_KIDS", env["UGOITE_DEV_SIGNING_KID"])


def test_dev_auth_req_ops_015_config_exposes_manual_totp_mode(
    monkeypatch: pytest.MonkeyPatch,
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
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual-totp login issues a bearer token for protected APIs."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
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
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: mock-oauth login remains explicit and returns a signed token."""
    timestamp = int(time.time())
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
        overrides={
            "UGOITE_DEV_USER_ID": "dev-oauth-user",
            "UGOITE_DEV_SIGNING_SECRET": "{}-{}-{}".format(
                "oauth",
                "signing",
                "secret",
            ),
        },
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
        overrides={"UGOITE_DEV_USER_ID": "   "},
    )
    clear_auth_manager_cache()

    response = TestClient(app).get("/auth/dev/config")

    assert response.status_code == 503


def test_dev_auth_req_ops_015_manual_login_rejects_non_manual_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login endpoint rejects mock-oauth sessions."""
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
    """REQ-OPS-015: manual login rejects usernames outside the terminal context."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
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


def test_dev_auth_req_ops_015_rejects_remote_clients(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth endpoints reject non-loopback clients."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    clear_auth_manager_cache()

    client = TestClient(app, client=("198.51.100.20", 50000))
    response = client.get("/auth/dev/config")

    assert response.status_code == 403
    assert "loopback clients" in response.json()["detail"]


def test_dev_auth_req_ops_015_rejects_unknown_client_host(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth endpoints reject requests.

    Requests without a resolved client host must be rejected.
    """
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )

    def _missing_client_host(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "app.api.endpoints.auth.resolve_client_host",
        _missing_client_host,
    )
    clear_auth_manager_cache()

    response = TestClient(app).get("/auth/dev/config")

    assert response.status_code == 403
    assert "loopback clients" in response.json()["detail"]


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
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", TEST_TOTP_SECRET)
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
    secret = TEST_TOTP_SECRET
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
    secret = TEST_TOTP_SECRET
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
