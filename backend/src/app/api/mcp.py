"""MCP (Model Context Protocol) server implementation.

This module implements the MCP server for IEapp, providing:
- Resources: Read access to notes, history, schema, and links
- Tools: CRUD operations for notes, search, and Python code execution

Spec Reference: 04_api_and_mcp.md §2
"""

from __future__ import annotations

import json
import logging
import uuid
from typing import Any

import ieapp
from mcp.server.fastmcp import FastMCP

from app.core.config import get_root_path
from app.core.sandbox import SandboxErrorType, run_in_sandbox

logger = logging.getLogger(__name__)

mcp = FastMCP("IEapp")


# =============================================================================
# MCP Resources (Spec 04 §2 Resources)
# =============================================================================


@mcp.resource("ieapp://{workspace_id}/notes/list")
def list_notes_resource(workspace_id: str) -> str:
    """List all notes in the workspace.

    Returns JSON list of notes with id, title, class, tags, and canvas_position.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    try:
        notes = ieapp.list_notes(str(ws_path))
        return json.dumps(notes, default=str)
    except Exception as e:
        logger.exception("Failed to list notes for workspace %s", workspace_id)
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/notes/{note_id}")
def get_note_resource(workspace_id: str, note_id: str) -> str:
    """Get note content (markdown body).

    Returns the raw markdown content of the note.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    try:
        note = ieapp.get_note(str(ws_path), note_id)
        # Return the markdown content as the resource body
        return note.get("markdown", "")
    except FileNotFoundError:
        return f"Error: Note '{note_id}' not found"
    except Exception as e:
        logger.exception("Failed to get note %s", note_id)
        return f"Error: {e}"


@mcp.resource("ieapp://{workspace_id}/notes/{note_id}/history")
def get_note_history_resource(workspace_id: str, note_id: str) -> str:
    """Get revision history for a note.

    Returns JSON list of revision summaries for Time Travel or restoration flows.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    try:
        history = ieapp.get_note_history(str(ws_path), note_id)
        return json.dumps(history, default=str)
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except Exception as e:
        logger.exception("Failed to get history for note %s", note_id)
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/schema")
def get_schema_resource(workspace_id: str) -> str:
    """Get available properties and values (all used tags, types, classes).

    Returns JSON object with aggregated schema information from the index.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    try:
        # Use Indexer to get notes, then aggregate stats
        indexer = ieapp.Indexer(str(ws_path))
        indexer.run_once()  # Ensure index is up to date

        # Read the generated stats from index
        stats_path = ws_path / "index" / "stats.json"
        stats = json.loads(stats_path.read_text()) if stats_path.exists() else {}
        return json.dumps(stats, default=str)
    except Exception as e:
        logger.exception("Failed to get schema for workspace %s", workspace_id)
        return json.dumps({"error": str(e)})


@mcp.resource("ieapp://{workspace_id}/links")
def get_links_resource(workspace_id: str) -> str:
    """Get canvas graph edges (source, target, metadata).

    Returns JSON list of all links between notes.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id
    try:
        notes = ieapp.list_notes(str(ws_path))
        # Aggregate links from all notes
        all_links: list[dict[str, Any]] = []
        for note in notes:
            note_links = note.get("links", [])
            for link in note_links:
                # Add source context
                link_data = {
                    "source": note["id"],
                    "target": link.get("target"),
                    "kind": link.get("kind", "related"),
                }
                all_links.append(link_data)
        return json.dumps(all_links, default=str)
    except Exception as e:
        logger.exception("Failed to get links for workspace %s", workspace_id)
        return json.dumps({"error": str(e)})


# =============================================================================
# MCP Tools (Spec 04 §2 Tools)
# =============================================================================


@mcp.tool()
def run_python_script(code: str, workspace_id: str) -> str:
    """Execute a Python script in the context of the workspace.

    The script has access to the `ieapp` library to manipulate notes.

    Args:
        code: The Python code to execute
        workspace_id: The target workspace

    Returns:
        Script output (stdout) or error message

    Example:
        ```python
        import ieapp

        tasks = ieapp.query(Class="task", status="pending")
        for task in tasks:
            print(f"- {task['title']}")
        ```

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return f"Error: Workspace '{workspace_id}' not found"

    env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
    result = run_in_sandbox(code, env=env)

    if result.error_type == SandboxErrorType.TIMEOUT:
        return f"Error: Script execution timed out\n{result.stderr}"
    if result.error_type == SandboxErrorType.SECURITY_VIOLATION:
        return f"Error: Security violation - {result.stderr}"
    if result.error_type == SandboxErrorType.MEMORY_EXCEEDED:
        return "Error: Memory limit exceeded"
    if result.returncode != 0:
        return f"Error (Exit Code {result.returncode}):\n{result.stderr}"
    return result.stdout


@mcp.tool()
def search_notes(query: str, workspace_id: str) -> str:
    """Semantic search for notes.

    Args:
        query: Search query string
        workspace_id: The target workspace

    Returns:
        JSON list of matching notes

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        # For now, do a simple text-based search on note content
        # Full semantic search (FAISS) is planned for Milestone 6
        notes = ieapp.list_notes(str(ws_path))
        results = []

        query_lower = query.lower()
        for note in notes:
            # Search in title and content
            title = note.get("title", "").lower()
            if query_lower in title:
                results.append(note)
                continue

            # Also check note content
            try:
                full_note = ieapp.get_note(str(ws_path), note["id"])
                content = full_note.get("markdown", "").lower()
                if query_lower in content:
                    results.append(note)
            except FileNotFoundError:
                # Skip notes that can't be read
                continue

        return json.dumps(results, default=str)
    except Exception as e:
        logger.exception("Failed to search notes in workspace %s", workspace_id)
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_list(
    workspace_id: str,
    filter_dict: dict[str, Any] | None = None,
) -> str:
    """List notes with optional filtering.

    Returns paginated note summaries (id, title, class, tags, canvas position)
    sourced from index/index.json.

    Args:
        workspace_id: Target workspace
        filter_dict: Optional filter (same shape as REST /query filters)

    Returns:
        JSON list of note summaries

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        if filter_dict:
            notes = ieapp.query_index(str(ws_path), filter_dict=filter_dict)
        else:
            notes = ieapp.list_notes(str(ws_path))
        return json.dumps(notes, default=str)
    except Exception as e:
        logger.exception("Failed to list notes for workspace %s", workspace_id)
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_read(workspace_id: str, note_id: str) -> str:
    """Fetch a full note payload.

    Returns frontmatter, markdown, attachments, and latest revision id.

    Args:
        workspace_id: Target workspace
        note_id: Note identifier

    Returns:
        JSON object with full note data

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        note = ieapp.get_note(str(ws_path), note_id)
        return json.dumps(note, default=str)
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except Exception as e:
        logger.exception("Failed to read note %s", note_id)
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_create(
    workspace_id: str,
    title: str,
    markdown: str,
    note_class: str | None = None,
    tags: list[str] | None = None,
) -> str:
    """Create a new note from raw markdown or a schema template.

    Args:
        workspace_id: Target workspace
        title: Note title
        markdown: Markdown content
        note_class: Optional class/schema name
        tags: Optional list of tags

    Returns:
        JSON object with created note info

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        note_id = str(uuid.uuid4())

        # Build frontmatter if class or tags provided
        frontmatter_lines = []
        if note_class or tags:
            frontmatter_lines.append("---")
            if note_class:
                frontmatter_lines.append(f"class: {note_class}")
            if tags:
                tags_str = ", ".join(tags)
                frontmatter_lines.append(f"tags: [{tags_str}]")
            frontmatter_lines.append("---")
            frontmatter_lines.append("")

        # Add title as H1 if not already present
        if not markdown.strip().startswith("# "):
            markdown = f"# {title}\n\n{markdown}"

        full_content = "\n".join(frontmatter_lines) + markdown

        ieapp.create_note(str(ws_path), note_id, full_content)

        return json.dumps({"id": note_id, "title": title, "status": "created"})
    except ieapp.NoteExistsError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        logger.exception("Failed to create note")
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_update(  # noqa: PLR0913
    workspace_id: str,
    note_id: str,
    parent_revision_id: str,
    markdown: str,
    frontmatter: dict[str, Any] | None = None,  # noqa: ARG001
    canvas_position: dict[str, Any] | None = None,  # noqa: ARG001
    tags: list[str] | None = None,  # noqa: ARG001
) -> str:
    """Update an existing note.

    Mirrors REST PUT semantics with optimistic concurrency via parent_revision_id.
    Properties are extracted from Markdown headers, not updated directly.

    Args:
        workspace_id: Target workspace
        note_id: Note identifier
        parent_revision_id: Required for optimistic concurrency
        markdown: New markdown content
        frontmatter: Optional frontmatter updates
        canvas_position: Optional canvas position updates
        tags: Optional tag updates

    Returns:
        JSON object with update status

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        # Note: frontmatter, canvas_position, and tags are not yet fully processed
        # The primary update mechanism is through markdown content
        ieapp.update_note(str(ws_path), note_id, markdown, parent_revision_id)

        # Get updated note info
        note = ieapp.get_note(str(ws_path), note_id)
        return json.dumps(
            {
                "id": note_id,
                "revision_id": note.get("revision_id"),
                "status": "updated",
            },
        )
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except ieapp.RevisionMismatchError as e:
        return json.dumps({"error": str(e), "conflict": True})
    except Exception as e:
        logger.exception("Failed to update note %s", note_id)
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_delete(workspace_id: str, note_id: str) -> str:
    """Tombstone a note (soft delete).

    The UI can confirm before purging.

    Args:
        workspace_id: Target workspace
        note_id: Note identifier

    Returns:
        JSON object with deletion status

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        ieapp.delete_note(str(ws_path), note_id)
        return json.dumps({"id": note_id, "status": "deleted"})
    except FileNotFoundError:
        return json.dumps({"error": f"Note '{note_id}' not found"})
    except Exception as e:
        logger.exception("Failed to delete note %s", note_id)
        return json.dumps({"error": str(e)})


@mcp.tool()
def notes_restore(workspace_id: str, note_id: str, revision_id: str) -> str:
    """Restore a past revision, creating a new head revision.

    Supports the Time Travel UI and agent-driven undo flows.

    Args:
        workspace_id: Target workspace
        note_id: Note identifier
        revision_id: Revision to restore

    Returns:
        JSON object with restore status and new revision id

    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        return json.dumps({"error": f"Workspace '{workspace_id}' not found"})

    try:
        result = ieapp.restore_note(str(ws_path), note_id, revision_id)
        return json.dumps(
            {
                "id": note_id,
                "restored_from": revision_id,
                "new_revision_id": result.get("new_revision_id"),
                "status": "restored",
            },
        )
    except FileNotFoundError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        logger.exception(
            "Failed to restore note %s to revision %s",
            note_id,
            revision_id,
        )
        return json.dumps({"error": str(e)})
