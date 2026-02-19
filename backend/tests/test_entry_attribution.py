"""Planned entry attribution tests.

REQ-FORM-009: Mandatory Attribution Metadata.
"""

import pytest


@pytest.mark.skip(reason="Milestone 4 identity-backed attribution not implemented yet")
def test_system_sets_author_and_updated_by_from_identity() -> None:
    """REQ-FORM-009: system sets author/updated_by from authenticated identity."""


@pytest.mark.skip(reason="Milestone 4 identity-backed attribution not implemented yet")
def test_client_cannot_override_author_or_updated_by() -> None:
    """REQ-FORM-009: client cannot forge author/updated_by metadata."""
