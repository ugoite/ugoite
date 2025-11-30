"""MCP (Model Context Protocol) server implementation.

This module implements MCP resources and tools as specified in 04_api_and_mcp.md:
- Resources: notes.list, notes.read, schema, links
- Tools: run_python_script, search_notes, notes.list/read/create/update/delete

Follows the MCP specification and uses FastMCP SDK for implementation.
"""

from __future__ import annotations

import json
import logging
import uuid
from typing import Annotated, Any

import ieapp
from ieapp.sandbox import (
    SandboxError,
    SandboxSecurityError,
    SandboxTimeoutError,
    run_python_script,
)
from mcp.server.fastmcp import FastMCP
from pydantic import Field

from app.core.config import get_root_path

logger = logging.getLogger(__name__)

# Create the MCP server with JSON response mode for better compatibility
mcp = FastMCP(
    name="ieapp",
    instructions="""IEapp MCP Server - A programmable knowledge base.

You can:
1. List and read notes using resources
2. Create, update, delete notes using tools
3. Execute Python code to analyze and manipulate notes
4. Query structured data extracted from Markdown headers

Use run_python_script for complex operations like batch updates,
data analysis, or custom queries.""",
    json_response=True,
)


# ============================================================================
# Resources - Read-only access to data
# ============================================================================


@mcp.resource("ieapp://{workspace_id}/notes/list")
def get_notes_list(workspace_id: str) -> str:
    """Get JSON list of all notes in a workspace.

    Returns note summaries including id, title, class, tags, and canvas position.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        notes = ieapp.list_notes(ws_path)
        return json.dumps(notes, indent=2, default=str)
    except Exception as e:
        logger.exception("Failed to list notes")
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/notes/{note_id}")
def get_note_content(workspace_id: str, note_id: str) -> str:
    """Get markdown content of a specific note.

    Returns the full note including frontmatter, markdown, attachments,
    and the latest revision_id for optimistic concurrency control.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        note = ieapp.get_note(ws_path, note_id)
        return json.dumps(note, indent=2, default=str)
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except Exception as e:
        logger.exception("Failed to get note")
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/notes/{note_id}/history")
def get_note_history(workspace_id: str, note_id: str) -> str:
    """Get revision history of a note for time travel features."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        history = ieapp.get_note_history(ws_path, note_id)
        return json.dumps(history, indent=2, default=str)
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except Exception as e:
        logger.exception("Failed to get note history")
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/schema")
def get_workspace_schema(workspace_id: str) -> str:
    """Get available properties and values (all used tags, types, classes)."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        # Load index.json to get notes dict for aggregate_stats
        index_path = ws_path / "index" / "index.json"
        if not index_path.exists():
            return json.dumps({"note_count": 0, "class_stats": {}, "tag_counts": {}})

        with index_path.open("r", encoding="utf-8") as f:
            index_data = json.load(f)

        notes = index_data.get("notes", {})
        stats = ieapp.aggregate_stats(notes)
        return json.dumps(stats, indent=2, default=str)
    except Exception as e:
        logger.exception("Failed to get schema")
        return json.dumps({"error": str(e)})


# ============================================================================
# Tools - Actions that can modify data
# ============================================================================


@mcp.tool()
def notes_list(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    filter_class: Annotated[
        str | None,
        Field(description="Filter by note class"),
    ] = None,
) -> dict[str, Any]:
    """List all notes in a workspace with optional filtering.

    Returns paginated note summaries sourced from index/index.json.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        notes = ieapp.list_notes(ws_path)

        # Apply filter if specified
        if filter_class:
            notes = [n for n in notes if n.get("class") == filter_class]

        return {"notes": notes, "count": len(notes)}
    except Exception as e:
        logger.exception("Failed to list notes")
        return {"error": str(e)}


@mcp.tool()
def notes_read(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    note_id: Annotated[str, Field(description="The note ID to read")],
) -> dict[str, Any]:
    """Fetch a full note payload (frontmatter, markdown, attachments, revision_id)."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        return ieapp.get_note(ws_path, note_id)
    except FileNotFoundError:
        return {"error": f"Note '{note_id}' not found"}
    except Exception as e:
        logger.exception("Failed to read note")
        return {"error": str(e)}


@mcp.tool()
def notes_create(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    title: Annotated[str, Field(description="Note title")],
    markdown: Annotated[str, Field(description="Markdown content")],
    note_class: Annotated[str | None, Field(description="Optional note class")] = None,
    tags: Annotated[list[str] | None, Field(description="Optional tags")] = None,
) -> dict[str, Any]:
    """Create a new note from raw markdown or a schema template."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    note_id = str(uuid.uuid4())

    # Build content with frontmatter if class/tags provided
    content = markdown
    if note_class or tags:
        frontmatter_parts = []
        if note_class:
            frontmatter_parts.append(f"class: {note_class}")
        if tags:
            frontmatter_parts.append(f"tags: [{', '.join(tags)}]")
        frontmatter = "\n".join(frontmatter_parts)
        content = f"---\n{frontmatter}\n---\n\n{markdown}"

    # Add title as H1 if not present
    if not content.strip().startswith("#"):
        content = f"# {title}\n\n{content}"

    try:
        ieapp.create_note(ws_path, note_id, content)
    except ieapp.NoteExistsError:
        return {"error": "Note already exists"}
    except Exception as e:
        logger.exception("Failed to create note")
        return {"error": str(e)}
    else:
        return {"id": note_id, "status": "created"}


@mcp.tool()
def notes_update(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    note_id: Annotated[str, Field(description="The note ID to update")],
    parent_revision_id: Annotated[
        str,
        Field(description="Parent revision ID for optimistic concurrency"),
    ],
    markdown: Annotated[str, Field(description="New markdown content")],
) -> dict[str, Any]:
    """Update an existing note.

    Requires parent_revision_id for optimistic concurrency control.
    Properties are extracted from Markdown headers (e.g., ## Date).
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        ieapp.update_note(
            ws_path,
            note_id,
            markdown,
            parent_revision_id=parent_revision_id,
        )
        # Get the updated note to return the new revision_id
        updated_note = ieapp.get_note(ws_path, note_id)
        return {
            "note_id": note_id,
            "revision_id": updated_note["revision_id"],
            "status": "updated",
        }
    except ieapp.RevisionMismatchError as e:
        return {"error": "Conflict: revision has changed", "details": str(e)}
    except FileNotFoundError:
        return {"error": f"Note '{note_id}' not found"}
    except Exception as e:
        logger.exception("Failed to update note")
        return {"error": str(e)}


@mcp.tool()
def notes_delete(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    note_id: Annotated[str, Field(description="The note ID to delete")],
) -> dict[str, Any]:
    """Tombstone a note (soft delete) for confirmation before purging."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        ieapp.delete_note(ws_path, note_id)
    except FileNotFoundError:
        return {"error": f"Note '{note_id}' not found"}
    except Exception as e:
        logger.exception("Failed to delete note")
        return {"error": str(e)}
    else:
        return {"id": note_id, "status": "deleted"}


@mcp.tool()
def notes_restore(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    note_id: Annotated[str, Field(description="The note ID to restore")],
    revision_id: Annotated[str, Field(description="The revision ID to restore")],
) -> dict[str, Any]:
    """Restore a past revision, creating a new head revision for auditing."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        result = ieapp.restore_note(ws_path, note_id, revision_id)
    except FileNotFoundError:
        return {"error": f"Note '{note_id}' or revision '{revision_id}' not found"}
    except Exception as e:
        logger.exception("Failed to restore note")
        return {"error": str(e)}
    else:
        return result


@mcp.tool()
def run_python_script_tool(
    code: Annotated[str, Field(description="Python code to execute")],
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
) -> dict[str, Any]:
    r"""Execute Python code in a sandboxed environment.

    The script has access to the `ieapp` library to manipulate notes:
    - ieapp.list_notes() - List all notes
    - ieapp.get_note(id) - Get a specific note
    - ieapp.query(class="meeting", status="open") - Query structured data
    - ieapp.update_note(id, content) - Update a note

    Sandbox restrictions:
    - Network access: Blocked
    - Filesystem: Restricted to workspace and /tmp
    - Timeout: 30 seconds
    - Libraries: ieapp, json, datetime, math, pandas, numpy

    Example:
    ```python
    import ieapp

    tasks = ieapp.query(class="task", status="pending")
    report = "# Pending Tasks\n"
    for task in tasks:
        due = task.properties.get('Due', 'N/A')
        report += f"- [ ] {task.title} (Due: {due})\n"
    print(report)
    ```

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        result = run_python_script(code, ws_path)
        return result.to_dict()
    except SandboxSecurityError as e:
        return {
            "error": "Security violation",
            "error_type": "SecurityError",
            "error_message": str(e),
            "success": False,
        }
    except SandboxTimeoutError as e:
        return {
            "error": "Script execution timed out",
            "error_type": "TimeoutError",
            "error_message": str(e),
            "success": False,
        }
    except SandboxError as e:
        return {
            "error": "Sandbox error",
            "error_type": "SandboxError",
            "error_message": str(e),
            "success": False,
        }
    except Exception as e:
        logger.exception("Failed to execute script")
        return {"error": str(e), "success": False}


@mcp.tool()
def search_notes(
    workspace_id: Annotated[str, Field(description="The target workspace ID")],
    query: Annotated[str, Field(description="Search query string")],
) -> dict[str, Any]:
    """Semantic search for notes (hybrid: vector + keyword).

    Currently implements keyword search; vector search will be added
    when FAISS integration is complete.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return {"error": f"Workspace '{workspace_id}' not found"}

    try:
        # Basic keyword search implementation
        notes = ieapp.list_notes(ws_path)
        query_lower = query.lower()

        results = []
        for note in notes:
            # Search in title and content
            title = note.get("title", "").lower()
            if query_lower in title:
                results.append(note)
                continue

            # Get full content for deeper search
            try:
                full_note = ieapp.get_note(ws_path, note["id"])
                content = full_note.get("markdown", "").lower()
                if query_lower in content:
                    results.append(note)
            except (FileNotFoundError, KeyError, OSError):
                pass  # Skip notes that can't be read

        return {"results": results, "count": len(results), "query": query}
    except Exception as e:
        logger.exception("Search failed")
        return {"error": str(e)}


# ============================================================================
# Prompts - Reusable interaction templates
# ============================================================================


@mcp.prompt(title="Summarize Workspace")
def summarize_workspace(workspace_id: str) -> str:
    """Generate a prompt to summarize workspace contents."""
    return f"""Read the index of workspace '{workspace_id}' and provide a summary.

Use the notes_list tool to get all notes, then analyze:
1. The main themes and categories
2. The total number of notes by class
3. Any patterns in the tags or properties
4. Suggestions for organization improvements"""


@mcp.prompt(title="Analyze Meetings")
def analyze_meetings(workspace_id: str) -> str:
    """Generate a prompt to analyze meeting notes."""
    return f"""Find meeting notes in workspace '{workspace_id}' and summarize them.

Use the notes_list tool with filter_class='meeting', then for each note:
1. Extract the date, attendees, and agenda
2. Identify action items and decisions
3. Create a consolidated summary"""


@mcp.prompt(title="Clean Up Notes")
def clean_up_notes(workspace_id: str, folder: str = "") -> str:
    """Generate a prompt for agentic refactoring of notes."""
    folder_clause = f" in folder '{folder}'" if folder else ""
    return f"""Help clean up the notes{folder_clause} in workspace '{workspace_id}'.

Steps:
1. Use notes_list to get all notes
2. Identify duplicates or similar content
3. Find notes missing required headers for their class
4. Suggest merges or reorganization
5. Present changes for confirmation before executing"""


# Export the MCP app for mounting
def get_mcp_app() -> FastMCP:
    """Return the MCP FastMCP instance for mounting."""
    return mcp
