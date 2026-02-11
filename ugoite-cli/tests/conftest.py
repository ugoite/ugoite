"""Test configuration and fixtures."""

from collections.abc import Generator
from typing import Any

import fsspec
import pytest


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
