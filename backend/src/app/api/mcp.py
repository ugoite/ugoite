from mcp.server.fastmcp import FastMCP
import ieapp
import os
import json
from app.core.sandbox import run_in_sandbox
from app.core.config import get_root_path

mcp = FastMCP("IEapp")

@mcp.tool()
def run_python_script(code: str, workspace_id: str) -> str:
    """Executes a Python script in the context of the workspace.
    
    The script has access to the `ieapp` library.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    
    env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
    result = run_in_sandbox(code, env=env)
    
    if result.returncode != 0:
        return f"Error (Exit Code {result.returncode}):\n{result.stderr}"
    return result.stdout

@mcp.resource("ieapp://{workspace_id}/notes/list")
def list_notes_resource(workspace_id: str) -> str:
    """List all notes in the workspace."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    
    try:
        notes = ieapp.list_notes(str(ws_path))
        return json.dumps(notes, default=str)
    except Exception as e:
        return json.dumps({"error": str(e)})

@mcp.resource("ieapp://{workspace_id}/notes/{note_id}")
def get_note_resource(workspace_id: str, note_id: str) -> str:
    """Get note content."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    try:
        note = ieapp.get_note(str(ws_path), note_id)
        # Return the markdown content as the resource body
        return note.get("markdown", "")
    except Exception as e:
        return f"Error: {str(e)}"
