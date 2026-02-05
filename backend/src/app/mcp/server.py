"""MCP server for ieapp resources."""

import json
import logging

import ieapp_core
from mcp.server.fastmcp import FastMCP

from app.core.config import get_root_path
from app.core.storage import storage_config_from_root

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ieapp")


@mcp.resource("ieapp://{space_id}/entries/list")
async def list_entries(space_id: str) -> str:
    """List all entries in the space."""
    storage_config = storage_config_from_root(get_root_path())
    try:
        entries = await ieapp_core.list_entries(storage_config, space_id)
    except RuntimeError:
        entries = []
    return json.dumps(entries)
