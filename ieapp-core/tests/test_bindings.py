"""Tests for the Python bindings of ieapp-core."""

import pytest

import ieapp_core


@pytest.mark.asyncio
async def test_list_workspaces_binding() -> None:
    """Verify that we can call list_workspaces from Python."""
    # list_workspaces now returns a future and requires storage_config
    result = await ieapp_core.list_workspaces({"uri": "memory://"})
    assert isinstance(result, list)


@pytest.mark.asyncio
async def test_test_storage_connection_binding() -> None:
    """Verify that we can call test_storage_connection from Python."""
    # test_storage_connection now returns a future and requires storage_config
    result = await ieapp_core.test_storage_connection({"uri": "memory://"})
    assert result is True
