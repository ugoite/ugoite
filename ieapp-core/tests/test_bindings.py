"""Tests for the Python bindings of ieapp-core."""

import pytest

import ieapp_core


@pytest.mark.asyncio
async def test_list_spaces_binding() -> None:
    """Verify that we can call list_spaces from Python."""
    # list_spaces now returns a future and requires storage_config
    result = await ieapp_core.list_spaces({"uri": "memory://"})
    assert isinstance(result, list)


@pytest.mark.asyncio
async def test_test_storage_connection_binding() -> None:
    """Verify that we can call test_storage_connection from Python."""
    # test_storage_connection now returns a future and requires storage_config
    result = await ieapp_core.test_storage_connection({"uri": "memory://"})
    assert result["status"] == "ok"
