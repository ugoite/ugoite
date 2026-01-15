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


@mcp.resource("ieapp://{workspace_id}/notes/list")
async def list_notes(workspace_id: str) -> str:
    """List all notes in the workspace."""
    storage_config = storage_config_from_root(get_root_path())
    try:
        notes = await ieapp_core.list_notes(storage_config, workspace_id)
    except RuntimeError:
        notes = []
    return json.dumps(notes)
