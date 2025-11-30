"""Test configuration for ieapp-cli."""

import shutil

import pytest

# Check if nsjail is available
NSJAIL_AVAILABLE = shutil.which("nsjail") is not None

requires_nsjail = pytest.mark.skipif(
    not NSJAIL_AVAILABLE,
    reason="nsjail is not installed",
)
