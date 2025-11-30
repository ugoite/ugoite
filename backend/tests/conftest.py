"""Test configuration."""

import os
import tempfile
from collections.abc import Generator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from main import app


@pytest.fixture
def test_client() -> TestClient:
    """Create a test client."""
    return TestClient(app)


@pytest.fixture
def temp_workspace_root() -> Generator[Path, None, None]:
    """Create a temporary workspace root."""
    with tempfile.TemporaryDirectory() as tmpdir:
        root = Path(tmpdir)
        os.environ["IEAPP_ROOT"] = str(root)
        yield root
        del os.environ["IEAPP_ROOT"]
