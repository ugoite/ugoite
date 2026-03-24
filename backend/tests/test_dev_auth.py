"""Local development login endpoint tests.

REQ-OPS-015: Local dev auth mode selection.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import os
import subprocess
import time
from pathlib import Path

import pytest
import ugoite_core
from fastapi.testclient import TestClient

from app.core.auth import clear_auth_manager_cache
from app.main import app

DEFAULT_TEST_USER_ID = "dev-alice"
DEFAULT_TEST_SIGNING_KID = "{}-{}-{}".format("dev", "local", "v1")
DEFAULT_TEST_SIGNING_SECRET = "{}-{}-{}".format("dev", "signing", "secret")
TEST_TOTP_SECRET = base64.b32encode(b"local-dev-auth-secret").decode("ascii")
REPO_ROOT = Path(__file__).resolve().parents[2]
DEV_SEED_SCRIPT_PATH = REPO_ROOT / "scripts" / "dev-seed.sh"


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
    response = client.get("/auth/config")

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
        "/auth/login",
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

    admin_space_response = client.get(
        f"/spaces/{ugoite_core.admin_space_id()}",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert admin_space_response.status_code == 200
    admin_settings = admin_space_response.json()["settings"]
    assert admin_settings["members"]["dev-alice"]["role"] == "admin"
    assert admin_settings["members"]["dev-alice"]["state"] == "active"


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
    login_response = client.post("/auth/mock-oauth")

    assert login_response.status_code == 200
    token = login_response.json()["bearer_token"]
    protected_response = client.get(
        "/spaces",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert protected_response.status_code == 200
    assert isinstance(protected_response.json(), list)

    admin_space_response = client.get(
        f"/spaces/{ugoite_core.admin_space_id()}",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert admin_space_response.status_code == 200
    admin_settings = admin_space_response.json()["settings"]
    assert admin_settings["members"]["dev-oauth-user"]["role"] == "admin"
    assert admin_settings["members"]["dev-oauth-user"]["state"] == "active"


def test_dev_auth_req_ops_015_startup_bootstraps_admin_space(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: app startup bootstraps admin-space for the configured dev user."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
        overrides={"UGOITE_DEV_USER_ID": "dev-startup-user"},
    )
    clear_auth_manager_cache()

    with TestClient(app) as client:
        login_response = client.post("/auth/mock-oauth")
        assert login_response.status_code == 200
        token = login_response.json()["bearer_token"]

        admin_space_response = client.get(
            f"/spaces/{ugoite_core.admin_space_id()}",
            headers={"Authorization": f"Bearer {token}"},
        )

    assert admin_space_response.status_code == 200
    admin_settings = admin_space_response.json()["settings"]
    assert admin_settings["members"]["dev-startup-user"]["state"] == "active"


def test_dev_seed_req_ops_016_seeded_space_is_visible_to_local_dev_stack(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
    tmp_path: Path,
) -> None:
    """REQ-OPS-016: seeded demo spaces stay visible to the local dev auth stack."""
    space_id = "dev-seed-visible"
    dev_user_id = "dev-seed-user"
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
        overrides={"UGOITE_DEV_USER_ID": dev_user_id},
    )
    clear_auth_manager_cache()

    seed_env = os.environ.copy()
    for key in list(seed_env):
        if key.startswith("UGOITE_SEED_"):
            del seed_env[key]
    seed_env.update(
        {
            "HOME": str(tmp_path),
            "CARGO_HOME": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
            "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")),
            "UGOITE_CLI_CONFIG_PATH": str(tmp_path / "cli-config.json"),
            "UGOITE_DEV_AUTH_MODE": "mock-oauth",
            "UGOITE_DEV_USER_ID": dev_user_id,
            "UGOITE_DEV_SIGNING_KID": DEFAULT_TEST_SIGNING_KID,
            "UGOITE_DEV_SIGNING_SECRET": DEFAULT_TEST_SIGNING_SECRET,
            "UGOITE_AUTH_BEARER_SECRETS": (
                f"{DEFAULT_TEST_SIGNING_KID}:{DEFAULT_TEST_SIGNING_SECRET}"
            ),
            "UGOITE_AUTH_BEARER_ACTIVE_KIDS": DEFAULT_TEST_SIGNING_KID,
            "UGOITE_ROOT": str(temp_space_root),
        },
    )

    seed_result = subprocess.run(
        [
            "/bin/bash",
            str(DEV_SEED_SCRIPT_PATH),
            "--space-id",
            space_id,
            "--scenario",
            "lab-qa",
            "--entry-count",
            "5",
            "--seed",
            "7",
        ],
        cwd=REPO_ROOT,
        env=seed_env,
        capture_output=True,
        text=True,
        timeout=300,
        check=False,
    )

    assert seed_result.returncode == 0, seed_result.stderr
    assert f"  root: {temp_space_root}" in seed_result.stderr
    assert (
        f"Verified seeded local sample space at {temp_space_root / 'spaces' / space_id}"
    ) in seed_result.stderr

    with TestClient(app) as client:
        login_response = client.post("/auth/mock-oauth")
        assert login_response.status_code == 200
        token = login_response.json()["bearer_token"]
        headers = {"Authorization": f"Bearer {token}"}

        list_response = client.get("/spaces", headers=headers)
        assert list_response.status_code == 200
        assert any(space["id"] == space_id for space in list_response.json()), (
            list_response.json()
        )

        space_response = client.get(f"/spaces/{space_id}", headers=headers)
        assert space_response.status_code == 200
        assert space_response.json()["id"] == space_id


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

    response = TestClient(app).get("/auth/config")

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

    response = TestClient(app).get("/auth/config")

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
        "/auth/login",
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
        "/auth/login",
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
    response = client.get("/auth/config")

    assert response.status_code == 403
    assert (
        response.json()["detail"]
        == "Explicit login endpoints are only available from loopback clients."
    )


def test_dev_auth_req_ops_015_allows_trusted_proxy_token(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth config accepts trusted frontend proxy requests."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    monkeypatch.setenv("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
    clear_auth_manager_cache()

    client = TestClient(app, client=("198.51.100.20", 50000))
    response = client.get(
        "/auth/config",
        headers={"x-ugoite-dev-auth-proxy-token": "proxy-secret"},
    )

    assert response.status_code == 200
    assert response.json()["mode"] == "mock-oauth"


def test_dev_auth_req_ops_015_rejects_invalid_trusted_proxy_token(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth config rejects untrusted proxy tokens."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    monkeypatch.setenv("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
    clear_auth_manager_cache()

    client = TestClient(app, client=("198.51.100.20", 50000))
    response = client.get(
        "/auth/config",
        headers={"x-ugoite-dev-auth-proxy-token": "wrong-secret"},
    )

    assert response.status_code == 403
    assert (
        response.json()["detail"]
        == "Explicit login endpoints are only available from loopback clients."
    )


def test_dev_auth_req_ops_015_rejects_missing_trusted_proxy_token(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth config rejects remote proxy requests without the token."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    monkeypatch.setenv("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
    clear_auth_manager_cache()

    client = TestClient(app, client=("198.51.100.20", 50000))
    response = client.get("/auth/config")

    assert response.status_code == 403
    assert (
        response.json()["detail"]
        == "Explicit login endpoints are only available from loopback clients."
    )


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

    response = TestClient(app).get("/auth/config")

    assert response.status_code == 403
    assert (
        response.json()["detail"]
        == "Explicit login endpoints are only available from loopback clients."
    )


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
        "/auth/login",
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
        "/auth/login",
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
        "/auth/login",
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
        "/auth/login",
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

    response = TestClient(app).post("/auth/mock-oauth")

    assert response.status_code == 409
    assert "mock-oauth login is not enabled" in response.json()["detail"]
