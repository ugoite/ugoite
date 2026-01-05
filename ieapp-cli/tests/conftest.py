"""Test configuration and fixtures."""

import uuid
from collections.abc import Generator
from typing import Any

import fsspec
import pytest


@pytest.fixture(params=["memory", "file"])
def fs_impl(
    request: pytest.FixtureRequest,
    tmp_path: Any,
) -> Generator[tuple[fsspec.AbstractFileSystem, str]]:
    """Fixture to provide different fsspec filesystem implementations."""
    protocol = request.param
    if protocol == "memory":
        # Use a fresh memory filesystem for each test
        fs = fsspec.filesystem("memory", skip_instance_cache=True)
        # Ensure we start clean
        fs.store.clear()
        root = f"/test_root_{uuid.uuid4().hex}"
        yield fs, root
        # Cleanup
        fs.store.clear()
    elif protocol == "file":
        fs = fsspec.filesystem("file")
        root = str(tmp_path / "test_root")
        yield fs, root
        # Cleanup handled by tmp_path
