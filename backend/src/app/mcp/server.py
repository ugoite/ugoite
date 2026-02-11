"""MCP server for ugoite resources."""

import json
import logging

import ugoite_core
from mcp.server.fastmcp import FastMCP

from app.core.config import get_root_path
from app.core.storage import storage_config_from_root

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ugoite")


@mcp.resource("ugoite://{space_id}/entries/list")
async def list_entries(space_id: str) -> str:
    """List all entries in the space."""
    storage_config = storage_config_from_root(get_root_path())
    try:
        entries = await ugoite_core.list_entries(storage_config, space_id)
    except RuntimeError:
        entries = []
    return json.dumps(entries)
