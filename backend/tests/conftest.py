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

from app.main import app  # noqa: E402


@pytest.fixture
def test_client(temp_workspace_root: Path) -> TestClient:  # noqa: ARG001
    """Create a test client bound to the temporary workspace root."""
    return TestClient(app)


@pytest.fixture
def temp_workspace_root() -> Iterator[Path]:
    """Create a temporary workspace root."""
    with tempfile.TemporaryDirectory() as tmpdir:
        root = Path(tmpdir)
        os.environ["IEAPP_ROOT"] = str(root)
        yield root
        del os.environ["IEAPP_ROOT"]
