"""Planned authentication enforcement tests.

REQ-SEC-003: Mandatory User Authentication.
"""

import pytest


@pytest.mark.skip(reason="Milestone 4 authentication middleware not implemented yet")
def test_auth_rejects_unauthenticated_localhost_requests() -> None:
    """REQ-SEC-003: localhost requests require authenticated user identity."""


@pytest.mark.skip(reason="Milestone 4 authentication middleware not implemented yet")
def test_auth_rejects_unauthenticated_remote_requests() -> None:
    """REQ-SEC-003: remote requests require authenticated user identity."""
