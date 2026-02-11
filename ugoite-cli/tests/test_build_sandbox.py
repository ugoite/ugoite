"""Sandbox build tests removed (sandbox feature deprecated)."""

import pytest

pytest.skip(
    "Sandbox build tooling was removed; build_sandbox tests are no longer applicable.",
    allow_module_level=True,
)
