"""MCP server for ieapp sandbox execution."""

import json
import logging

from ieapp.sandbox import run_script
from mcp.server.fastmcp import FastMCP

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ieapp")


@mcp.tool()
async def run_script_tool(code: str, workspace_id: str) -> str:
    """Execute a JavaScript script in a secure WebAssembly sandbox.

    The script has access to the host application's REST API via 'host.call'.

    Args:
        code: The JavaScript code to execute.
        workspace_id: The target workspace ID.

    """
    logger.info("Executing script for workspace %s", workspace_id)

    def host_call_handler(method: str, path: str, body: dict | None) -> dict[str, str]:
        # TODO(ieapp): Implement actual API call dispatch
        logger.info("Host call: %s %s", method, path)
        del body  # Unused for now
        return {"status": "mock_response", "method": method, "path": path}

    try:
        result = run_script(code, host_call_handler)
        return json.dumps(result, indent=2)
    except Exception as e:
        return f"Error: {e}"


@mcp.resource("ieapp://{workspace_id}/notes/list")
async def list_notes(workspace_id: str) -> str:
    """List all notes in the workspace."""
    del workspace_id  # Unused for now
    return json.dumps([])
