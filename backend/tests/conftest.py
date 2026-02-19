"""Test configuration."""

import os
import sys
import tempfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SRC_ROOT = PROJECT_ROOT / "src"
if str(SRC_ROOT) not in sys.path:
    sys.path.insert(0, str(SRC_ROOT))

from app.core.auth import clear_auth_manager_cache
from app.main import app

TEST_AUTH_TOKEN = "test-suite-token"


@pytest.fixture(autouse=True)
def configure_auth_env(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """Configure deterministic auth settings for tests."""
    monkeypatch.setenv("UGOITE_BOOTSTRAP_BEARER_TOKEN", TEST_AUTH_TOKEN)
    monkeypatch.setenv("UGOITE_BOOTSTRAP_USER_ID", "test-suite-user")
    clear_auth_manager_cache()
    yield
    clear_auth_manager_cache()


@pytest.fixture
def test_client(temp_space_root: Path) -> TestClient:
    """Create a test client bound to the temporary space root."""
    return TestClient(app, headers={"Authorization": f"Bearer {TEST_AUTH_TOKEN}"})


@pytest.fixture
def temp_space_root() -> Iterator[Path]:
    """Create a temporary space root."""
    with tempfile.TemporaryDirectory() as tmpdir:
        root = Path(tmpdir)
        os.environ["UGOITE_ROOT"] = str(root)
        yield root
        del os.environ["UGOITE_ROOT"]
