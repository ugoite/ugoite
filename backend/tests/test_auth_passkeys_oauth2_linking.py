"""Planned passkey and OAuth2 linking tests.

REQ-SEC-004: Passkey-First Authentication and OAuth2 Linking.
"""

import pytest


@pytest.mark.skip(reason="Milestone 4 passkey flow not implemented yet")
def test_passkey_primary_authentication__planned() -> None:
    """REQ-SEC-004: passkey (WebAuthn) is the primary authentication method."""


@pytest.mark.skip(reason="Milestone 4 OAuth2 account linking not implemented yet")
def test_oauth2_account_linking__planned() -> None:
    """REQ-SEC-004: OAuth2 accounts can be linked to space-scoped users."""
