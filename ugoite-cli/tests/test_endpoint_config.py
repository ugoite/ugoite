"""Tests for CLI endpoint routing config."""

from pathlib import Path

import pytest

from ugoite.endpoint_config import (
    EndpointConfig,
    encode_path_component,
    endpoint_config_path,
    load_endpoint_config,
    parse_space_id,
    request_json,
    resolve_base_url,
    save_endpoint_config,
)


def test_endpoint_config_roundtrip_uses_home_directory(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-001: Persists endpoint config under ~/.ugoite and loads it back."""
    monkeypatch.setenv("HOME", str(tmp_path))

    config = EndpointConfig(
        mode="api",
        backend_url="http://b:8000",
        api_url="http://f:3000/api",
    )
    saved = save_endpoint_config(config)

    assert saved == endpoint_config_path()
    assert saved.parent.name == ".ugoite"
    loaded = load_endpoint_config()
    assert loaded.mode == "api"
    assert loaded.backend_url == "http://b:8000"
    assert loaded.api_url == "http://f:3000/api"


def test_parse_space_id_from_path_and_id() -> None:
    """REQ-STO-004: Handles both full space path and raw space id values."""
    assert parse_space_id("root/spaces/default") == "default"
    assert parse_space_id("default") == "default"


def test_parse_space_id_rejects_empty_values() -> None:
    """REQ-STO-004: Rejects empty or whitespace-only space identifiers."""
    with pytest.raises(ValueError, match="must not be empty"):
        parse_space_id("")
    with pytest.raises(ValueError, match="must not be empty"):
        parse_space_id("   ")


def test_encode_path_component_escapes_reserved_characters() -> None:
    """REQ-STO-004: Encodes path segments before remote URL composition."""
    assert encode_path_component("a/b c") == "a%2Fb%20c"


def test_resolve_base_url_by_mode() -> None:
    """REQ-STO-001: Returns correct base URL for each mode."""
    assert resolve_base_url(EndpointConfig(mode="core")) is None
    assert (
        resolve_base_url(
            EndpointConfig(
                mode="backend",
                backend_url="http://localhost:8000/",
            ),
        )
        == "http://localhost:8000"
    )
    assert (
        resolve_base_url(
            EndpointConfig(
                mode="api",
                api_url="http://localhost:3000/api/",
            ),
        )
        == "http://localhost:3000/api"
    )


def test_request_json_closes_connection_on_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-004: HTTP connection is closed even when request fails."""
    closed: dict[str, bool] = {"value": False}

    class FakeConn:
        def __init__(self, _netloc: str, timeout: int) -> None:
            assert timeout == 30

        def request(self, *_args: object, **_kwargs: object) -> None:
            msg = "boom"
            raise OSError(msg)

        def close(self) -> None:
            closed["value"] = True

    monkeypatch.setattr("ugoite.endpoint_config.http.client.HTTPConnection", FakeConn)

    with pytest.raises(RuntimeError, match="Connection failed"):
        request_json("GET", "http://localhost:8000/spaces")

    assert closed["value"] is True


def test_request_json_preserves_structured_http_error_fields(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-004: request_json surfaces code/action from structured error payloads."""

    class FakeResponse:
        status = 403

        def read(self) -> bytes:
            return (
                b'{"detail":{"detail":"Forbidden by policy","code":"forbidden",'
                b'"action":"entry_write"}}'
            )

    class FakeConnection:
        def __init__(self, _netloc: str, timeout: int) -> None:
            _ = timeout

        def request(
            self,
            _method: str,
            _path: str,
            *,
            body: str | None,
            headers: dict[str, str],
        ) -> None:
            assert body is None
            assert headers["Accept"] == "application/json"

        def getresponse(self) -> FakeResponse:
            return FakeResponse()

        def close(self) -> None:
            return None

    monkeypatch.setattr(
        "ugoite.endpoint_config.http.client.HTTPConnection",
        FakeConnection,
    )

    with pytest.raises(RuntimeError, match="code=forbidden") as exc_info:
        request_json("GET", "http://localhost:8000/spaces")

    assert "action=entry_write" in str(exc_info.value)


def test_request_json_retries_idempotent_methods_with_backoff(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-004: request_json retries transient GET failures with backoff."""
    attempts: list[int] = []

    class FakeResponse:
        status = 200

        def read(self) -> bytes:
            return b'{"ok": true}'

    class FakeConnection:
        def __init__(self, _netloc: str, timeout: int) -> None:
            _ = timeout

        def request(
            self,
            _method: str,
            _path: str,
            *,
            body: str | None,
            headers: dict[str, str],
        ) -> None:
            assert body is None
            assert headers["Accept"] == "application/json"
            attempts.append(1)
            if len(attempts) < 3:
                msg = "transient"
                raise OSError(msg)

        def getresponse(self) -> FakeResponse:
            return FakeResponse()

        def close(self) -> None:
            return None

    sleeps: list[float] = []

    monkeypatch.setattr(
        "ugoite.endpoint_config.http.client.HTTPConnection",
        FakeConnection,
    )

    def _record_sleep(seconds: float) -> None:
        sleeps.append(seconds)

    monkeypatch.setattr(
        "ugoite.endpoint_config.time.sleep",
        _record_sleep,
    )

    assert request_json("GET", "http://localhost:8000/spaces") == {"ok": True}
    assert len(attempts) == 3
    assert sleeps == [0.1, 0.2]
