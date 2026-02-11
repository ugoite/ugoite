"""Sandbox tests removed (sandbox feature deprecated).

REQ-SEC-001: Localhost binding (sandbox deprecated entry).
"""

import pytest

pytest.skip(
    "Sandbox execution was removed; sandbox tests are no longer applicable.",
    allow_module_level=True,
)
