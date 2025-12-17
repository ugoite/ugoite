"""MCP Server implementation."""

import json
import logging
from typing import Any

from mcp.server.fastmcp import FastMCP

from app.sandbox.python_sandbox import run_script

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

    def host_call_handler(method: str, path: str, body: dict | None) -> Any:  # noqa: ANN401
        # TODO: Implement actual API call dispatch  # noqa: TD002, TD003, FIX002
        logger.info("Host call: %s %s (body: %s)", method, path, body)
        return {"status": "mock_response", "method": method, "path": path}

    try:
        result = run_script(code, host_call_handler)
        return json.dumps(result, indent=2)
    except Exception as e:  # noqa: BLE001
        return f"Error: {e!s}"


@mcp.resource("ieapp://{workspace_id}/notes/list")
async def list_notes(workspace_id: str) -> str:
    """List all notes in the workspace."""
    logger.info("Listing notes for workspace %s", workspace_id)
    return json.dumps([])
