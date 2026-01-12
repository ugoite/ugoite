"""Tests for ieapp-core Python bindings.

Note: These tests currently verify mock implementations. They should be expanded
to cover real business logic and edge cases as the core moves beyond mocks.
"""

import ieapp_core


def test_list_workspaces_binding() -> None:
    """Verify that we can call list_workspaces from Python."""
    result = ieapp_core.list_workspaces()
    assert isinstance(result, list)
    assert "mock_workspace" in result


def test_test_storage_connection_binding() -> None:
    """Verify that we can call test_storage_connection from Python."""
    result = ieapp_core.test_storage_connection()
    assert result is True
