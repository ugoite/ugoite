"""Test configuration and fixtures."""

from collections.abc import Generator
from typing import Any

import fsspec
import pytest


@pytest.fixture(autouse=True)
def isolate_cli_user_config(
    tmp_path: Any,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-001: isolate CLI endpoint config per test run."""
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.delenv("UGOITE_CLI_CONFIG_PATH", raising=False)
    monkeypatch.delenv("UGOITE_CONFIG_HOME", raising=False)
    monkeypatch.delenv("XDG_CONFIG_HOME", raising=False)


@pytest.fixture(params=["file"])
def fs_impl(
    request: pytest.FixtureRequest,
    tmp_path: Any,
) -> Generator[tuple[fsspec.AbstractFileSystem, str]]:
    """Fixture to provide different fsspec filesystem implementations."""
    protocol = request.param
    if protocol == "file":
        fs = fsspec.filesystem("file")
        root = str(tmp_path / "test_root")
        yield fs, root
        # Cleanup handled by tmp_path
