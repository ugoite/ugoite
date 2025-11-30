import logging
from typing import Any, Dict, Optional
from mcp.server.fastmcp import FastMCP
from app.sandbox.python_sandbox import run_script
import json

logger = logging.getLogger(__name__)

# Initialize FastMCP
mcp = FastMCP("ieapp")


@mcp.tool()
async def run_script_tool(code: str, workspace_id: str) -> str:
    """
    Executes a JavaScript script in a secure WebAssembly sandbox.
    The script has access to the host application's REST API via 'host.call'.

    Args:
        code: The JavaScript code to execute.
        workspace_id: The target workspace ID.
    """
    logger.info(f"Executing script for workspace {workspace_id}")

    def host_call_handler(method: str, path: str, body: Optional[Dict]) -> Any:
        # TODO: Implement actual API call dispatch
        logger.info(f"Host call: {method} {path}")
        return {"status": "mock_response", "method": method, "path": path}

    try:
        result = run_script(code, host_call_handler)
        return json.dumps(result, indent=2)
    except Exception as e:
        return f"Error: {str(e)}"


@mcp.resource("ieapp://{workspace_id}/notes/list")
async def list_notes(workspace_id: str) -> str:
    """List all notes in the workspace."""
    return json.dumps([])
