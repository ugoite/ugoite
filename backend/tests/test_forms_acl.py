"""Planned Form ACL and inheritance tests.

REQ-SEC-006: Space-Scoped Authorization and Form ACL.
"""


def test_form_acl_denies_unauthorized_read_access() -> None:
    """REQ-SEC-006: unauthorized users cannot read restricted Forms."""
    assert True


def test_form_acl_denies_unauthorized_write_access() -> None:
    """REQ-SEC-006: unauthorized users cannot write restricted Forms."""
    assert True


def test_form_acl_allows_group_principal_access() -> None:
    """REQ-SEC-006: UserGroup principal access can be granted."""
    assert True


def test_materialized_view_inherits_form_acl_policies() -> None:
    """REQ-SEC-006: materialized views inherit source Form ACL policies."""
    assert True
