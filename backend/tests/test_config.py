"""Tests for backend root path resolution.

REQ-STO-001: Storage abstraction defaults to user-owned runtime path.
"""

from pathlib import Path

import pytest

from app.core.config import get_root_path


def test_get_root_path_defaults_to_home_runtime(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REQ-STO-001: backend default root path MUST be ~/.ugoite when env is unset."""
    monkeypatch.delenv("UGOITE_ROOT", raising=False)
    assert get_root_path() == Path.home() / ".ugoite"


def test_get_root_path_prefers_env_override(monkeypatch: pytest.MonkeyPatch) -> None:
    """REQ-STO-001: backend MUST honor explicit UGOITE_ROOT override."""
    custom_root = "/workspace/custom-root"
    monkeypatch.setenv("UGOITE_ROOT", custom_root)
    assert get_root_path() == custom_root
