"""Local development login endpoint tests.

REQ-OPS-015: Local dev auth mode selection.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import io
import os
import shutil
import subprocess
import threading
import time
from collections.abc import Callable, Iterator
from contextlib import contextmanager, redirect_stderr
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
import ugoite_core
from fastapi.testclient import TestClient

from app.api.endpoints import auth as auth_endpoints
from app.core.auth import clear_auth_manager_cache
from app.main import app

DEFAULT_TEST_USER_ID = "dev-alice"
DEFAULT_TEST_SIGNING_KID = "{}-{}-{}".format("dev", "local", "v1")
DEFAULT_TEST_SIGNING_SECRET = "{}-{}-{}".format("dev", "signing", "secret")
DEFAULT_TEST_PASSKEY_CONTEXT = "{}-{}-{}".format("dev", "passkey", "context")
TEST_TOTP_SECRET = base64.b32encode(b"local-dev-auth-secret").decode("ascii")
REPO_ROOT = Path(__file__).resolve().parents[2]
DEV_SEED_SCRIPT_PATH = REPO_ROOT / "scripts" / "dev-seed.sh"
LOCAL_SMOKE_CHECK_SCRIPT_PATH = REPO_ROOT / "scripts" / "local-smoke-check.sh"


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
        "UGOITE_DEV_PASSKEY_CONTEXT": DEFAULT_TEST_PASSKEY_CONTEXT,
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


@contextmanager
def _serve_http(
    response_factory: Callable[[str, dict[str, str]], tuple[int, bytes, str]],
) -> Iterator[tuple[str, list[tuple[str, dict[str, str]]]]]:
    requests: list[tuple[str, dict[str, str]]] = []

    class _Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            headers = dict(self.headers.items())
            requests.append((self.path, headers))
            status_code, body, content_type = response_factory(self.path, headers)
            self.send_response(status_code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 0), _Handler)

    def _serve_quietly() -> None:
        with redirect_stderr(io.StringIO()):
            server.serve_forever()

    thread = threading.Thread(target=_serve_quietly, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", requests
    finally:
        server.shutdown()
        thread.join()
        server.server_close()


def _run_local_smoke_check(
    *,
    backend_url: str,
    frontend_url: str,
    bearer_token: str | None = None,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.update(
        {
            "BACKEND_URL": backend_url,
            "FRONTEND_URL": frontend_url,
        },
    )
    env.pop("UGOITE_AUTH_API_KEY", None)
    if bearer_token is None:
        env.pop("UGOITE_AUTH_BEARER_TOKEN", None)
    else:
        env["UGOITE_AUTH_BEARER_TOKEN"] = bearer_token
    return subprocess.run(
        ["/bin/bash", str(LOCAL_SMOKE_CHECK_SCRIPT_PATH)],
        cwd=REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )


def _passkey_headers(
    context: str = DEFAULT_TEST_PASSKEY_CONTEXT,
) -> dict[str, str]:
    return {"x-ugoite-dev-passkey-context": context}


@pytest.fixture(autouse=True)
def _clear_dev_login_attempts() -> Iterator[None]:
    auth_endpoints.clear_login_attempts()
    yield
    auth_endpoints.clear_login_attempts()


def test_dev_auth_req_ops_015_config_exposes_passkey_totp_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: local browser/CLI login can discover passkey-totp mode."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    clear_auth_manager_cache()

    client = TestClient(app)
    response = client.get("/auth/config")

    assert response.status_code == 200
    assert response.json() == {
        "mode": "passkey-totp",
        "username_hint": "dev-alice",
        "supports_passkey_totp": True,
        "supports_mock_oauth": False,
    }


def test_dev_auth_req_ops_015_passkey_totp_login_issues_signed_token(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login issues a bearer token for protected APIs."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", "3600")
    monkeypatch.setattr("ugoite_core.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    client = TestClient(app)
    login_response = client.post(
        "/auth/login",
        json={
            "username": "dev-alice",
            "totp_code": _totp_code(secret, timestamp),
        },
        headers=_passkey_headers(),
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


def test_dev_auth_req_ops_015_passkey_totp_login_rejects_replayed_code(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login rejects replaying the same current 2FA code."""
    timestamp = 1_700_000_000
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", "3600")
    monkeypatch.setattr("ugoite_core.auth.time.time", lambda: timestamp)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    client = TestClient(app)
    payload = {
        "username": "dev-alice",
        "totp_code": _totp_code(secret, timestamp),
    }

    first_response = client.post(
        "/auth/login",
        json=payload,
        headers=_passkey_headers(),
    )
    second_response = client.post(
        "/auth/login",
        json=payload,
        headers=_passkey_headers(),
    )

    assert first_response.status_code == 200
    assert second_response.status_code == 401
    assert second_response.json()["detail"] == "Invalid username or 2FA code."


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
    monkeypatch.setattr("ugoite_core.auth.time.time", lambda: timestamp)
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


def test_dev_auth_req_ops_015_auth_config_remains_read_only(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: auth config discovery must not recreate admin-space."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
        overrides={"UGOITE_DEV_USER_ID": "dev-config-user"},
    )
    clear_auth_manager_cache()

    admin_space_dir = temp_space_root / "spaces" / ugoite_core.admin_space_id()

    with TestClient(app) as client:
        assert admin_space_dir.is_dir()
        shutil.rmtree(admin_space_dir)

        response = client.get("/auth/config")

    assert response.status_code == 200
    assert response.json()["mode"] == "mock-oauth"
    assert not admin_space_dir.exists()


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


def test_local_smoke_check_req_ops_015_uses_auth_config_without_token() -> None:
    """REQ-OPS-015: local smoke checks stay non-interactive without a bearer token."""

    def backend_response(path: str, _headers: dict[str, str]) -> tuple[int, bytes, str]:
        if path == "/health":
            return 200, b"ok", "text/plain; charset=utf-8"
        if path == "/auth/config":
            return 200, b'{"mode":"mock-oauth"}', "application/json"
        if path == "/spaces":
            return 401, b'{"detail":"missing bearer token"}', "application/json"
        return 404, b"not found", "text/plain; charset=utf-8"

    def frontend_response(
        _path: str,
        _headers: dict[str, str],
    ) -> tuple[int, bytes, str]:
        return 200, b"<!doctype html><title>Ugoite</title>", "text/html; charset=utf-8"

    with (
        _serve_http(backend_response) as (backend_url, backend_requests),
        _serve_http(
            frontend_response,
        ) as (frontend_url, frontend_requests),
    ):
        result = _run_local_smoke_check(
            backend_url=backend_url,
            frontend_url=frontend_url,
        )

    assert result.returncode == 0, result.stderr
    assert [path for path, _headers in backend_requests] == ["/health", "/auth/config"]
    assert [path for path, _headers in frontend_requests] == ["/"]
    assert "Checking backend auth config" in result.stdout
    assert "Local smoke check passed" in result.stdout


def test_local_smoke_check_req_ops_015_fails_when_auth_config_errors() -> None:
    """REQ-OPS-015: local smoke checks fail when the auth config probe fails."""

    def backend_response(path: str, _headers: dict[str, str]) -> tuple[int, bytes, str]:
        if path == "/health":
            return 200, b"ok", "text/plain; charset=utf-8"
        if path == "/auth/config":
            return 503, b'{"detail":"auth config unavailable"}', "application/json"
        return 404, b"not found", "text/plain; charset=utf-8"

    def frontend_response(
        _path: str,
        _headers: dict[str, str],
    ) -> tuple[int, bytes, str]:
        return 200, b"<!doctype html><title>Ugoite</title>", "text/html; charset=utf-8"

    with (
        _serve_http(backend_response) as (backend_url, backend_requests),
        _serve_http(
            frontend_response,
        ) as (frontend_url, frontend_requests),
    ):
        result = _run_local_smoke_check(
            backend_url=backend_url,
            frontend_url=frontend_url,
        )

    assert result.returncode != 0
    assert [path for path, _headers in backend_requests] == ["/health", "/auth/config"]
    assert frontend_requests == []
    assert "Checking backend auth config" in result.stdout
    assert "503" in result.stderr


def test_local_smoke_check_req_ops_015_uses_spaces_with_bearer_token() -> None:
    """REQ-OPS-015: local smoke checks may probe spaces when auth is explicit."""
    bearer_token = "dev-local-token"

    def backend_response(path: str, headers: dict[str, str]) -> tuple[int, bytes, str]:
        if path == "/health":
            return 200, b"ok", "text/plain; charset=utf-8"
        if path == "/auth/config":
            return 200, b'{"mode":"mock-oauth"}', "application/json"
        if path == "/spaces":
            if headers.get("Authorization") == f"Bearer {bearer_token}":
                return 200, b"[]", "application/json"
            return 401, b'{"detail":"missing bearer token"}', "application/json"
        return 404, b"not found", "text/plain; charset=utf-8"

    def frontend_response(
        _path: str,
        _headers: dict[str, str],
    ) -> tuple[int, bytes, str]:
        return 200, b"<!doctype html><title>Ugoite</title>", "text/html; charset=utf-8"

    with (
        _serve_http(backend_response) as (backend_url, backend_requests),
        _serve_http(
            frontend_response,
        ) as (frontend_url, frontend_requests),
    ):
        result = _run_local_smoke_check(
            backend_url=backend_url,
            frontend_url=frontend_url,
            bearer_token=bearer_token,
        )

    assert result.returncode == 0, result.stderr
    assert [path for path, _headers in backend_requests] == ["/health", "/spaces"]
    assert [path for path, _headers in frontend_requests] == ["/"]
    assert backend_requests[1][1]["Authorization"] == f"Bearer {bearer_token}"
    assert "Checking backend spaces endpoint" in result.stdout
    assert "Local smoke check passed" in result.stdout


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
        mode="passkey-totp",
        overrides={"UGOITE_DEV_USER_ID": "   "},
    )
    clear_auth_manager_cache()

    response = TestClient(app).get("/auth/config")

    assert response.status_code == 503


def test_dev_auth_req_ops_015_passkey_login_rejects_non_passkey_mode(
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
    assert "passkey-totp login is not enabled" in response.json()["detail"]


def test_dev_auth_req_ops_015_passkey_login_rejects_wrong_username(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login rejects usernames outside the terminal context."""
    timestamp = int(time.time())
    monotonic = 30_000.0
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    monkeypatch.setattr("app.api.endpoints.auth.time.monotonic", lambda: monotonic)
    clear_auth_manager_cache()

    client = TestClient(app)
    payload = {"username": "other-user", "totp_code": _totp_code(secret, timestamp)}

    for _ in range(auth_endpoints.LOGIN_FAILURE_LIMIT - 1):
        response = client.post(
            "/auth/login",
            json=payload,
            headers=_passkey_headers(),
        )
        assert response.status_code == 401
        assert response.json()["detail"] == "Invalid username or 2FA code."
        monotonic += 1.0

    throttled_response = client.post(
        "/auth/login",
        json=payload,
        headers=_passkey_headers(),
    )

    assert throttled_response.status_code == 429
    assert throttled_response.json()["detail"] == (
        "Too many failed login attempts. Try again in 60 seconds."
    )


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


def test_dev_auth_req_ops_015_rejects_spoofed_loopback_forwarded_for(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: dev auth ignores spoofed loopback forwarded headers."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="mock-oauth",
    )
    monkeypatch.setenv("UGOITE_ALLOW_REMOTE", "true")
    monkeypatch.setenv("UGOITE_TRUST_PROXY_HEADERS", "true")
    clear_auth_manager_cache()

    client = TestClient(app, client=("198.51.100.20", 50000))
    response = client.get("/auth/config", headers={"x-forwarded-for": "127.0.0.1"})

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


def test_dev_auth_req_ops_015_passkey_login_requires_totp_secret(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login requires a configured local 2FA secret."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.delenv("UGOITE_DEV_2FA_SECRET", raising=False)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": "123456"},
        headers=_passkey_headers(),
    )

    assert response.status_code == 503


def test_dev_auth_req_ops_015_passkey_login_rejects_invalid_totp_code(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: manual login rejects invalid 2FA codes."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", TEST_TOTP_SECRET)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": "000000"},
        headers=_passkey_headers(),
    )

    assert response.status_code == 401
    assert response.json()["detail"] == "Invalid username or 2FA code."


def test_dev_auth_req_ops_015_passkey_login_throttles_repeated_failures(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: repeated invalid passkey login attempts temporarily return 429."""
    monotonic = 10_000.0
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", TEST_TOTP_SECRET)
    monkeypatch.setattr("app.api.endpoints.auth.time.monotonic", lambda: monotonic)
    clear_auth_manager_cache()

    client = TestClient(app)
    invalid_payload = {"username": "dev-alice", "totp_code": "000000"}

    for _ in range(auth_endpoints.LOGIN_FAILURE_LIMIT - 1):
        response = client.post(
            "/auth/login",
            json=invalid_payload,
            headers=_passkey_headers(),
        )
        assert response.status_code == 401
        assert response.json()["detail"] == "Invalid username or 2FA code."
        monotonic += 1.0

    throttled_response = client.post(
        "/auth/login",
        json=invalid_payload,
        headers=_passkey_headers(),
    )

    assert throttled_response.status_code == 429
    assert throttled_response.json()["detail"] == (
        "Too many failed login attempts. Try again in 60 seconds."
    )
    assert throttled_response.headers["retry-after"] == "60"

    follow_up_response = client.post(
        "/auth/login",
        json=invalid_payload,
        headers=_passkey_headers(),
    )

    assert follow_up_response.status_code == 429
    assert follow_up_response.headers["retry-after"] == "60"


@pytest.mark.parametrize(
    ("headers", "expected_detail"),
    [
        (None, "Passkey-bound local context is missing or invalid."),
        (
            _passkey_headers("wrong-context"),
            "Passkey-bound local context is missing or invalid.",
        ),
        (
            _passkey_headers("   "),
            "Passkey-bound local context is missing or invalid.",
        ),
    ],
    ids=["missing", "wrong", "blank"],
)
def test_dev_auth_req_ops_015_passkey_login_throttles_repeated_context_failures(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
    headers: dict[str, str] | None,
    expected_detail: str,
) -> None:
    """REQ-OPS-015: passkey-totp login throttles repeated local-context failures."""
    timestamp = 1_700_000_000
    monotonic = 15_000.0
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    monkeypatch.setattr("app.api.endpoints.auth.time.monotonic", lambda: monotonic)
    clear_auth_manager_cache()

    client = TestClient(app)
    valid_payload = {
        "username": "dev-alice",
        "totp_code": _totp_code(secret, timestamp),
    }

    for _ in range(auth_endpoints.LOGIN_FAILURE_LIMIT - 1):
        if headers is None:
            response = client.post("/auth/login", json=valid_payload)
        else:
            response = client.post(
                "/auth/login",
                json=valid_payload,
                headers=headers,
            )
        assert response.status_code == 401
        assert response.json()["detail"] == expected_detail
        monotonic += 1.0

    if headers is None:
        throttled_response = client.post("/auth/login", json=valid_payload)
    else:
        throttled_response = client.post(
            "/auth/login",
            json=valid_payload,
            headers=headers,
        )
    assert throttled_response.status_code == 429
    assert throttled_response.json()["detail"] == (
        "Too many failed login attempts. Try again in 60 seconds."
    )
    assert throttled_response.headers["retry-after"] == "60"


def test_dev_auth_req_ops_015_passkey_login_recovers_after_throttle_window(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login clears the temporary throttle after the wait."""
    timestamp = 1_700_000_000
    monotonic = 20_000.0
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", "3600")
    monkeypatch.setattr("ugoite_core.auth.time.time", lambda: timestamp)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    monkeypatch.setattr("app.api.endpoints.auth.time.monotonic", lambda: monotonic)
    clear_auth_manager_cache()

    client = TestClient(app)
    invalid_payload = {"username": "dev-alice", "totp_code": "000000"}

    for _ in range(auth_endpoints.LOGIN_FAILURE_LIMIT):
        response = client.post(
            "/auth/login",
            json=invalid_payload,
            headers=_passkey_headers(),
        )
        monotonic += 1.0

    assert response.status_code == 429
    monotonic += auth_endpoints.LOGIN_LOCKOUT_SECONDS + 1.0

    successful_response = client.post(
        "/auth/login",
        json={
            "username": "dev-alice",
            "totp_code": _totp_code(secret, timestamp),
        },
        headers=_passkey_headers(),
    )

    assert successful_response.status_code == 200

    monotonic += 1.0
    next_failure_response = client.post(
        "/auth/login",
        json=invalid_payload,
        headers=_passkey_headers(),
    )

    assert next_failure_response.status_code == 401
    assert next_failure_response.json()["detail"] == "Invalid username or 2FA code."


def test_dev_auth_req_ops_015_passkey_login_requires_signing_material(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey login cannot issue a token without signing material."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_SIGNING_SECRET", " ")
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", " ")
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
        headers=_passkey_headers(),
    )

    assert response.status_code == 503


@pytest.mark.parametrize(
    ("ttl_value", "expected_detail"),
    [
        ("abc", "UGOITE_DEV_AUTH_TTL_SECONDS must be an integer."),
        ("0", "UGOITE_DEV_AUTH_TTL_SECONDS must be positive."),
    ],
)
def test_dev_auth_req_ops_015_passkey_login_validates_ttl(
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
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setenv("UGOITE_DEV_AUTH_TTL_SECONDS", ttl_value)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
        headers=_passkey_headers(),
    )

    assert response.status_code == 503


def test_dev_auth_req_ops_015_mock_oauth_rejects_manual_totp_mode(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: mock-oauth endpoint rejects passkey-totp sessions."""
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    clear_auth_manager_cache()

    response = TestClient(app).post("/auth/mock-oauth")

    assert response.status_code == 409
    assert "mock-oauth login is not enabled" in response.json()["detail"]


def test_dev_auth_req_ops_015_passkey_login_rejects_missing_passkey_context(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login rejects requests without the local context."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
    )

    assert response.status_code == 401
    assert (
        response.json()["detail"]
        == "Passkey-bound local context is missing or invalid."
    )


def test_dev_auth_req_ops_015_passkey_login_rejects_invalid_passkey_context(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login rejects requests with the wrong local context."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
        headers=_passkey_headers("wrong-context"),
    )

    assert response.status_code == 401
    assert (
        response.json()["detail"]
        == "Passkey-bound local context is missing or invalid."
    )


def test_dev_auth_req_ops_015_passkey_login_rejects_blank_passkey_context(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login rejects blank local passkey context headers."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
        headers=_passkey_headers("   "),
    )

    assert response.status_code == 401
    assert (
        response.json()["detail"]
        == "Passkey-bound local context is missing or invalid."
    )


def test_dev_auth_req_ops_015_passkey_login_requires_configured_passkey_context(
    monkeypatch: pytest.MonkeyPatch,
    temp_space_root: Path,
) -> None:
    """REQ-OPS-015: passkey-totp login requires configured local passkey context."""
    timestamp = int(time.time())
    secret = TEST_TOTP_SECRET
    _configure_dev_auth_env(
        monkeypatch,
        temp_space_root,
        mode="passkey-totp",
    )
    monkeypatch.setenv("UGOITE_DEV_2FA_SECRET", secret)
    monkeypatch.delenv("UGOITE_DEV_PASSKEY_CONTEXT", raising=False)
    monkeypatch.setattr("app.api.endpoints.auth.time.time", lambda: timestamp)
    clear_auth_manager_cache()

    response = TestClient(app).post(
        "/auth/login",
        json={"username": "dev-alice", "totp_code": _totp_code(secret, timestamp)},
        headers=_passkey_headers(),
    )

    assert response.status_code == 503
    assert response.json()["detail"] == (
        "Failed to configure passkey-totp login: "
        "UGOITE_DEV_PASSKEY_CONTEXT must be configured."
    )
