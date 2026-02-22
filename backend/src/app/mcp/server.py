"""MCP server for ugoite resources."""

import json
import logging
from typing import Any

import ugoite_core
from mcp.server.fastmcp import Context, FastMCP
from ugoite_core.auth import authenticate_headers_for_space

from app.core.config import get_root_path
from app.core.storage import storage_config_from_root

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ugoite")


def _context_headers(
    ctx: Context[Any, Any, Any],
) -> tuple[
    dict[str, str] | object,
    str | None,
    str | None,
    str | None,
]:
    request = ctx.request_context.request
    if request is None:
        message = "Missing authentication context for MCP request"
        raise RuntimeError(message)

    headers = getattr(request, "headers", None)
    if headers is None:
        headers = request.get("headers") if isinstance(request, dict) else None
    if headers is None:
        message = "Missing request headers for MCP request"
        raise RuntimeError(message)

    request_method = getattr(request, "method", None)
    request_id = None
    request_path = None

    request_url = getattr(request, "url", None)
    if request_url is not None:
        request_path = getattr(request_url, "path", None)
    if request_path is None:
        request_path = getattr(request, "path", None)

    request_headers = headers if hasattr(headers, "get") else None
    if request_headers is not None:
        request_id = request_headers.get("x-request-id")

    return headers, request_method, request_path, request_id


@mcp.resource("ugoite://{space_id}/entries/list")
async def list_entries(space_id: str, ctx: Context[Any, Any, Any]) -> str:
    """List all entries in the space."""
    storage_config = storage_config_from_root(get_root_path())
    headers, request_method, request_path, request_id = _context_headers(ctx)
    identity = await authenticate_headers_for_space(
        storage_config,
        space_id,
        headers,
        request_method=request_method,
        request_path=request_path,
        request_id=request_id,
    )
    await ugoite_core.require_space_action(
        storage_config,
        space_id,
        identity,
        "entry_read",
    )
    entries = await ugoite_core.list_entries(storage_config, space_id)
    filtered_entries = await ugoite_core.filter_readable_entries(
        storage_config,
        space_id,
        identity,
        entries,
    )
    return json.dumps(filtered_entries)
