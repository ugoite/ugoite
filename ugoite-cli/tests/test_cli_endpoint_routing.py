"""Tests for CLI endpoint routing behavior."""

from pathlib import Path

import pytest
from typer.testing import CliRunner

from ugoite.cli import app
from ugoite.endpoint_config import EndpointConfig

runner = CliRunner()


def test_cli_config_set_and_show(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-001: Config commands persist settings under ~/.ugoite."""
    monkeypatch.setenv("HOME", str(tmp_path))

    result = runner.invoke(
        app,
        [
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:18000",
        ],
        catch_exceptions=False,
    )
    assert result.exit_code == 0
    assert "Saved endpoint config" in result.stdout

    show = runner.invoke(app, ["config", "show"], catch_exceptions=False)
    assert show.exit_code == 0
    assert '"mode": "backend"' in show.stdout
    assert "127.0.0.1:18000" in show.stdout


def test_space_list_uses_remote_endpoint_when_backend_mode(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-004: Space list routes to configured backend endpoint in remote mode."""
    monkeypatch.setattr(
        "ugoite.cli._endpoint_config",
        lambda: EndpointConfig(mode="backend"),
    )

    calls: list[tuple[str, str]] = []

    def fake_request_json(
        method: str,
        url: str,
        payload: dict[str, object] | None = None,
    ) -> list[dict[str, str]]:
        assert payload is None
        calls.append((method, url))
        return [{"id": "default", "name": "default"}]

    monkeypatch.setattr("ugoite.cli.request_json", fake_request_json)

    result = runner.invoke(app, ["space", "list", "root"], catch_exceptions=False)

    assert result.exit_code == 0
    assert calls == [("GET", "http://localhost:8000/spaces")]
    assert '"id": "default"' in result.stdout
