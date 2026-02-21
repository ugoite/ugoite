"""Authentication logging tests.

REQ-SEC-003: Mandatory User Authentication.
"""

from __future__ import annotations

import logging
import os
from typing import TYPE_CHECKING

from ugoite_core.auth import clear_auth_manager_cache, get_auth_manager

if TYPE_CHECKING:
    import pytest


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
    original = {key: os.environ.get(key) for key in env_keys}
    bootstrap_value = os.urandom(12).hex()

    try:
        for key in env_keys:
            os.environ.pop(key, None)
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
    finally:
        clear_auth_manager_cache()
        for key, value in original.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
