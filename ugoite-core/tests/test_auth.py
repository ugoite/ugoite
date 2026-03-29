"""Authentication logging tests.

REQ-SEC-003: Mandatory User Authentication.
REQ-OPS-015: Local dev auth mode selection.
"""

from __future__ import annotations

import base64
import hmac
import logging
import os

import pytest

from ugoite_core.auth import (
    AuthError,
    authenticate_headers,
    clear_auth_manager_cache,
    get_auth_manager,
    mint_signed_bearer_token,
    validate_totp_code,
)

TEST_TOTP_SECRET = base64.b32encode(b"local-dev-auth-secret").decode("ascii")


def test_bootstrap_warning_redacts_token_value(
    caplog: pytest.LogCaptureFixture,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: bootstrap token warning never logs raw token values."""
    env_keys = [
        "UGOITE_AUTH_BEARER_TOKENS_JSON",
        "UGOITE_BOOTSTRAP_BEARER_TOKEN",
        "UGOITE_AUTH_API_KEYS_JSON",
        "UGOITE_AUTH_API_KEYS",
        "UGOITE_AUTH_BEARER_SECRETS",
        "UGOITE_AUTH_BEARER_ACTIVE_KIDS",
        "UGOITE_AUTH_REVOKED_KEY_IDS",
    ]
    bootstrap_value = os.urandom(12).hex()

    for key in env_keys:
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setattr(
        "ugoite_core.auth.secrets.token_urlsafe",
        lambda _: bootstrap_value,
    )
    clear_auth_manager_cache()

    caplog.set_level(logging.WARNING, logger="ugoite_core.auth")
    get_auth_manager()

    messages = [record.getMessage() for record in caplog.records]
    joined = "\n".join(messages)
    assert "Generated one-time bootstrap token" in joined
    assert bootstrap_value not in joined
    assert "fingerprint=" in joined
    clear_auth_manager_cache()


def test_bearer_secrets_config_skips_bootstrap_warning(
    caplog: pytest.LogCaptureFixture,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: signed bearer-secret config must not trigger bootstrap fallback."""
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", "dev-local-v1:test-secret")
    monkeypatch.setenv("UGOITE_AUTH_BEARER_ACTIVE_KIDS", "dev-local-v1")
    monkeypatch.delenv("UGOITE_AUTH_BEARER_TOKENS_JSON", raising=False)
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    clear_auth_manager_cache()

    caplog.set_level(logging.WARNING, logger="ugoite_core.auth")
    manager = get_auth_manager()

    assert manager.bootstrap_token is None
    assert not caplog.records


def test_get_auth_manager_req_sec_003_refreshes_bootstrap_token_after_ttl(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: auth manager reloads bootstrap token after the TTL expires."""
    first_token = os.urandom(8).hex()
    second_token = os.urandom(8).hex()
    monkeypatch.delenv("UGOITE_AUTH_BEARER_TOKENS_JSON", raising=False)
    monkeypatch.delenv("UGOITE_AUTH_BEARER_SECRETS", raising=False)
    monkeypatch.setenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", first_token)
    clear_auth_manager_cache()

    monotonic_now = {"value": 100.0}
    monkeypatch.setattr(
        "ugoite_core.auth.time.monotonic",
        lambda: monotonic_now["value"],
    )

    first = get_auth_manager()
    assert first.bootstrap_token == first_token

    monkeypatch.setenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", second_token)
    second = get_auth_manager()
    assert second.bootstrap_token == first_token

    monotonic_now["value"] += 61.0
    third = get_auth_manager()
    assert third.bootstrap_token == second_token


def test_get_auth_manager_req_sec_003_keeps_generated_token_until_clear(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: generated bootstrap tokens stay stable until explicit clear."""
    first_generated = os.urandom(8).hex()
    second_generated = os.urandom(8).hex()
    generated_tokens = iter([first_generated, second_generated])
    monkeypatch.delenv("UGOITE_AUTH_BEARER_TOKENS_JSON", raising=False)
    monkeypatch.delenv("UGOITE_AUTH_BEARER_SECRETS", raising=False)
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    monkeypatch.setattr(
        "ugoite_core.auth.secrets.token_urlsafe",
        lambda _: next(generated_tokens),
    )
    clear_auth_manager_cache()

    monotonic_now = {"value": 300.0}
    monkeypatch.setattr(
        "ugoite_core.auth.time.monotonic",
        lambda: monotonic_now["value"],
    )

    first = get_auth_manager()
    assert first.bootstrap_token == first_generated

    monotonic_now["value"] += 61.0
    second = get_auth_manager()
    assert second.bootstrap_token == first_generated

    clear_auth_manager_cache()
    third = get_auth_manager()
    assert third.bootstrap_token == second_generated


def test_authenticate_headers_req_sec_003_rejects_revoked_key_after_cache_expiry(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-SEC-003: revoked bearer signing keys fail after TTL refresh."""
    secret = "test-" + "secret"
    monkeypatch.setenv("UGOITE_AUTH_BEARER_SECRETS", f"dev-local-v1:{secret}")
    monkeypatch.setenv("UGOITE_AUTH_BEARER_ACTIVE_KIDS", "dev-local-v1")
    monkeypatch.delenv("UGOITE_AUTH_BEARER_TOKENS_JSON", raising=False)
    monkeypatch.delenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", raising=False)
    monkeypatch.delenv("UGOITE_AUTH_REVOKED_KEY_IDS", raising=False)
    clear_auth_manager_cache()

    monotonic_now = {"value": 200.0}
    monkeypatch.setattr(
        "ugoite_core.auth.time.monotonic",
        lambda: monotonic_now["value"],
    )

    token = mint_signed_bearer_token(
        user_id="dev-user",
        key_id="dev-local-v1",
        secret=secret,
        expires_at=2_000_000_000,
    )

    identity = authenticate_headers({"authorization": f"Bearer {token}"})
    assert identity.user_id == "dev-user"

    monkeypatch.setenv("UGOITE_AUTH_REVOKED_KEY_IDS", "dev-local-v1")
    monotonic_now["value"] += 61.0

    with pytest.raises(AuthError) as excinfo:
        authenticate_headers({"authorization": f"Bearer {token}"})

    assert excinfo.value.code == "revoked_key"


def _totp_code(secret: str, timestamp: int) -> str:
    decoded_secret = base64.b32decode(secret.upper(), casefold=True)
    counter = timestamp // 30
    digest = hmac.new(
        decoded_secret,
        counter.to_bytes(8, "big"),
        "sha1",
    ).digest()
    offset = digest[-1] & 0x0F
    binary = int.from_bytes(digest[offset : offset + 4], "big") & 0x7FFFFFFF
    return f"{binary % 1_000_000:06d}"


def test_validate_totp_code_req_ops_015_accepts_current_code() -> None:
    """REQ-OPS-015: local manual-totp mode validates a current six-digit code."""
    secret = TEST_TOTP_SECRET
    timestamp = 1_700_000_000
    code = _totp_code(secret, timestamp)
    assert validate_totp_code(code, secret, now=timestamp)


def test_validate_totp_code_req_ops_015_accepts_unpadded_secret() -> None:
    """REQ-OPS-015: local manual-totp mode accepts unpadded Base32 secrets."""
    secret = TEST_TOTP_SECRET.rstrip("=")
    timestamp = 1_700_000_000
    code = _totp_code(TEST_TOTP_SECRET, timestamp)
    assert validate_totp_code(code, secret, now=timestamp)
