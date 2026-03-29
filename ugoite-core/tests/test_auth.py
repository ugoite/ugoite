"""Authentication logging tests.

REQ-SEC-003: Mandatory User Authentication.
REQ-OPS-015: Local dev auth mode selection.
"""

from __future__ import annotations

import base64
import hmac
import logging
import os
from typing import TYPE_CHECKING

from ugoite_core.auth import (
    clear_auth_manager_cache,
    get_auth_manager,
    validate_totp_code,
)

if TYPE_CHECKING:
    import pytest

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
    """REQ-OPS-015: local passkey-totp mode validates a current six-digit code."""
    secret = TEST_TOTP_SECRET
    timestamp = 1_700_000_000
    code = _totp_code(secret, timestamp)
    clear_auth_manager_cache()
    assert validate_totp_code(code, secret, now=timestamp)
    clear_auth_manager_cache()


def test_validate_totp_code_req_ops_015_accepts_unpadded_secret() -> None:
    """REQ-OPS-015: local passkey-totp mode accepts unpadded Base32 secrets."""
    secret = TEST_TOTP_SECRET.rstrip("=")
    timestamp = 1_700_000_000
    code = _totp_code(TEST_TOTP_SECRET, timestamp)
    clear_auth_manager_cache()
    assert validate_totp_code(code, secret, now=timestamp)
    clear_auth_manager_cache()


def test_validate_totp_code_req_ops_015_rejects_replayed_code() -> None:
    """REQ-OPS-015: local manual-totp mode rejects replaying the same TOTP code."""
    secret = TEST_TOTP_SECRET
    timestamp = 1_700_000_000
    code = _totp_code(secret, timestamp)
    clear_auth_manager_cache()
    assert validate_totp_code(code, secret, now=timestamp)
    assert not validate_totp_code(code, secret, now=timestamp)
    clear_auth_manager_cache()


def test_validate_totp_code_req_ops_015_accepts_next_time_step_after_replay() -> None:
    """REQ-OPS-015: local manual-totp mode still accepts the next valid TOTP step."""
    secret = TEST_TOTP_SECRET
    timestamp = 1_700_000_000
    next_timestamp = timestamp + 30
    current_code = _totp_code(secret, timestamp)
    next_code = _totp_code(secret, next_timestamp)
    clear_auth_manager_cache()
    assert validate_totp_code(current_code, secret, now=timestamp)
    assert not validate_totp_code(current_code, secret, now=timestamp)
    assert validate_totp_code(next_code, secret, now=next_timestamp)
    clear_auth_manager_cache()
