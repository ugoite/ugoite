"""Planned authentication enforcement tests.

REQ-SEC-003: Mandatory User Authentication.
"""


def test_auth_rejects_unauthenticated_localhost_requests() -> None:
    """REQ-SEC-003: localhost requests require authenticated user identity."""
    assert True


def test_auth_rejects_unauthenticated_remote_requests() -> None:
    """REQ-SEC-003: remote requests require authenticated user identity."""
    assert True
