"""MCP server for ieapp resources."""

import json
import logging

from mcp.server.fastmcp import FastMCP

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ieapp")


@mcp.resource("ieapp://{workspace_id}/notes/list")
async def list_notes(workspace_id: str) -> str:
    """List all notes in the workspace."""
    del workspace_id  # Unused for now
    return json.dumps([])
