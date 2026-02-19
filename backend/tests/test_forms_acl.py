"""Planned Form ACL and inheritance tests.

REQ-SEC-006: Space-Scoped Authorization and Form ACL.
"""

import pytest


@pytest.mark.skip(reason="Milestone 4 ACL enforcement not implemented yet")
def test_form_acl_denies_unauthorized_read_access() -> None:
    """REQ-SEC-006: unauthorized users cannot read restricted Forms."""


@pytest.mark.skip(reason="Milestone 4 ACL enforcement not implemented yet")
def test_form_acl_denies_unauthorized_write_access() -> None:
    """REQ-SEC-006: unauthorized users cannot write restricted Forms."""


@pytest.mark.skip(reason="Milestone 4 ACL enforcement not implemented yet")
def test_form_acl_allows_group_principal_access() -> None:
    """REQ-SEC-006: UserGroup principal access can be granted."""


@pytest.mark.skip(
    reason="Milestone 4 materialized view ACL inheritance not implemented yet",
)
def test_materialized_view_inherits_form_acl_policies() -> None:
    """REQ-SEC-006: materialized views inherit source Form ACL policies."""
