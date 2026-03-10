"""REQ-API-001: Portable preferences endpoints validate and persist user settings."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING

import ugoite_core

if TYPE_CHECKING:
    import pytest
    from fastapi.testclient import TestClient


def test_preferences_me_roundtrip(
    test_client: TestClient,
    temp_space_root: Path,
) -> None:
    """REQ-API-001: /preferences/me stores portable preferences.

    User paths are hashed rather than embedding raw user IDs.
    """
    response = test_client.get("/preferences/me")
    assert response.status_code == 200
    assert response.json() == {
        "selected_space_id": None,
        "locale": None,
        "ui_theme": None,
        "color_mode": None,
        "primary_color": None,
    }

    test_client.post("/spaces", json={"name": "portable-space"})

    payload = {
        "selected_space_id": "portable-space",
        "locale": "ja",
        "ui_theme": "classic",
        "color_mode": "dark",
        "primary_color": "blue",
    }
    patch_response = test_client.patch("/preferences/me", json=payload)
    assert patch_response.status_code == 200
    assert patch_response.json() == payload

    roundtrip = test_client.get("/preferences/me")
    assert roundtrip.status_code == 200
    assert roundtrip.json() == payload

    user_hash = hashlib.sha256(b"test-suite-user").hexdigest()
    preferences_path = temp_space_root / "users" / user_hash / "preferences.json"
    assert preferences_path.exists()
    assert "test-suite-user" not in str(preferences_path.relative_to(temp_space_root))

    stored = json.loads(preferences_path.read_text(encoding="utf-8"))
    assert stored == payload


def test_preferences_me_rejects_invalid_selected_space_id(
    test_client: TestClient,
) -> None:
    """REQ-API-001: /preferences/me rejects invalid selected_space_id values."""
    response = test_client.patch(
        "/preferences/me",
        json={"selected_space_id": "invalid space"},
    )
    assert response.status_code == 400
    assert "Invalid selected_space_id" in response.json()["detail"]


def test_preferences_me_accepts_null_selected_space_id(
    test_client: TestClient,
) -> None:
    """REQ-API-001: /preferences/me allows clearing selected_space_id with null."""
    response = test_client.patch(
        "/preferences/me",
        json={"selected_space_id": None},
    )
    assert response.status_code == 200
    assert response.json()["selected_space_id"] is None


def test_preferences_me_get_returns_explicit_error_on_core_failure(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-001: /preferences/me GET returns explicit errors on core failure."""

    async def _raise(*_args: object, **_kwargs: object) -> dict[str, object]:
        msg = "boom"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "get_user_preferences", _raise)

    response = test_client.get("/preferences/me")
    assert response.status_code == 500
    assert response.json()["detail"] == "Failed to load preferences"


def test_preferences_me_patch_returns_explicit_error_on_core_failure(
    test_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-API-001: /preferences/me PATCH returns explicit errors on core failure."""

    async def _raise(*_args: object, **_kwargs: object) -> dict[str, object]:
        msg = "boom"
        raise RuntimeError(msg)

    monkeypatch.setattr(ugoite_core, "patch_user_preferences", _raise)

    response = test_client.patch("/preferences/me", json={"locale": "ja"})
    assert response.status_code == 500
    assert response.json()["detail"] == "Failed to update preferences"
