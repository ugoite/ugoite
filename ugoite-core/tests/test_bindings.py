"""Tests for the Python bindings of ugoite-core."""

import socket
from unittest.mock import AsyncMock, patch

import pytest

import ugoite_core


@pytest.mark.asyncio
async def test_list_spaces_binding() -> None:
    """Verify that we can call list_spaces from Python."""
    # list_spaces now returns a future and requires storage_config
    result = await ugoite_core.list_spaces({"uri": "memory://"})
    assert isinstance(result, list)


@pytest.mark.asyncio
async def test_test_storage_connection_binding() -> None:
    """Verify that we can call test_storage_connection from Python."""
    # test_storage_connection now returns a future and requires storage_config
    result = await ugoite_core.test_storage_connection({"uri": "memory://"})
    assert result["status"] == "ok"


@pytest.mark.asyncio
async def test_test_storage_connection_req_sto_006_rejects_link_local_endpoint() -> (
    None
):
    """REQ-STO-006: reject link-local endpoints before calling the binding."""
    with (
        patch(
            "ugoite_core._core_any.test_storage_connection",
            AsyncMock(return_value={"status": "ok"}),
        ) as mock_core,
        pytest.raises(
            ValueError,
            match=(
                r"Storage endpoint host is not allowed: "
                r"169\.254\.169\.254"
            ),
        ),
    ):
        await ugoite_core.test_storage_connection(
            {
                "uri": "s3://bucket/path",
                "endpoint": "http://169.254.169.254/latest/meta-data/",
            },
        )
    mock_core.assert_not_awaited()


@pytest.mark.asyncio
async def test_test_storage_connection_req_sto_006_strips_inputs_before_validation(
) -> None:
    """REQ-STO-006: storage validation trims uri and endpoint before validation."""
    with patch(
        "ugoite_core._core_any.test_storage_connection",
        AsyncMock(return_value={"status": "ok"}),
    ) as mock_core:
        result = await ugoite_core.test_storage_connection(
            {
                "uri": "  fs:///tmp/storage  ",
                "endpoint": " https://storage.example.test ",
            },
        )

    assert result == {"status": "ok"}
    mock_core.assert_awaited_once_with(
        {
            "uri": "  fs:///tmp/storage  ",
            "endpoint": " https://storage.example.test ",
        },
    )


@pytest.mark.asyncio
async def test_storage_connection_req_sto_006_rejects_loopback_hostnames() -> None:
    """REQ-STO-006: endpoint hostnames resolving to loopback are rejected."""
    with (
        patch(
            "ugoite_core.storage_validation.socket.getaddrinfo",
            return_value=[
                (
                    socket.AF_INET,
                    socket.SOCK_STREAM,
                    6,
                    "",
                    ("127.0.0.1", 443),
                ),
            ],
        ),
        patch(
            "ugoite_core._core_any.test_storage_connection",
            AsyncMock(return_value={"status": "ok"}),
        ) as mock_core,
        pytest.raises(
            ValueError,
            match=r"Storage endpoint host is not allowed: storage\.example\.test",
        ),
    ):
        await ugoite_core.test_storage_connection(
            {
                "uri": "s3://bucket/path",
                "endpoint": "https://storage.example.test",
            },
        )

    mock_core.assert_not_awaited()
