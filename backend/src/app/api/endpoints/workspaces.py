"""Workspace endpoints."""

import logging
import uuid
from typing import Any

import ieapp
from fastapi import APIRouter, HTTPException, status
from ieapp.notes import (
    NoteExistsError,
    RevisionMismatchError,
    create_note,
    delete_note,
    get_note,
    get_note_history,
    get_note_revision,
    list_notes,
    restore_note,
    update_note,
)
from ieapp.workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
)

from app.core.config import get_root_path
from app.models.schemas import (
    NoteCreate,
    NoteRestore,
    NoteUpdate,
    QueryRequest,
    WorkspaceCreate,
)

router = APIRouter()
logger = logging.getLogger(__name__)


@router.get("/workspaces")
async def list_workspaces_endpoint() -> list[dict[str, Any]]:
    """List all workspaces."""
    root_path = get_root_path()

    try:
        return list_workspaces(root_path)
    except Exception as e:
        logger.exception("Failed to list workspaces")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces", status_code=status.HTTP_201_CREATED)
async def create_workspace_endpoint(
    payload: WorkspaceCreate,
) -> dict[str, str]:
    """Create a new workspace."""
    root_path = get_root_path()
    workspace_id = payload.name  # Using name as ID for now per simple spec

    try:
        create_workspace(root_path, workspace_id)
    except WorkspaceExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {
        "id": workspace_id,
        "name": payload.name,
        "path": str(root_path / "workspaces" / workspace_id),
    }


@router.get("/workspaces/{workspace_id}")
async def get_workspace_endpoint(workspace_id: str) -> dict[str, Any]:
    """Get workspace metadata."""
    root_path = get_root_path()

    try:
        return get_workspace(root_path, workspace_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post(
    "/workspaces/{workspace_id}/notes",
    status_code=status.HTTP_201_CREATED,
)
async def create_note_endpoint(
    workspace_id: str,
    payload: NoteCreate,
) -> dict[str, str]:
    """Create a new note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    note_id = payload.id or str(uuid.uuid4())

    try:
        create_note(ws_path, note_id, payload.content)
        # Get the created note to retrieve revision_id
        note_data = get_note(ws_path, note_id)
    except NoteExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {"id": note_id, "revision_id": note_data.get("revision_id", "")}


@router.get("/workspaces/{workspace_id}/notes")
async def list_notes_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all notes in a workspace."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        return list_notes(ws_path)
    except Exception as e:
        logger.exception("Failed to list notes")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/notes/{note_id}")
async def get_note_endpoint(workspace_id: str, note_id: str) -> dict[str, Any]:
    """Get a note by ID."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        return get_note(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.put("/workspaces/{workspace_id}/notes/{note_id}")
async def update_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteUpdate,
) -> dict[str, Any]:
    """Update an existing note.

    Requires parent_revision_id for optimistic concurrency control.
    Returns 409 Conflict if the revision has changed.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        update_note(
            ws_path,
            note_id,
            payload.markdown,
            payload.parent_revision_id,
        )
        # Return the updated note with id and revision_id
        updated_note = get_note(ws_path, note_id)
        return {
            "id": note_id,
            "revision_id": updated_note.get("revision_id", ""),
        }
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except RevisionMismatchError as e:
        # Return 409 with the current server version for client merge.
        # FastAPI supports dict as detail value, which serializes to JSON.
        # This allows clients to perform OCC merge with the current_revision.
        try:
            current_note = get_note(ws_path, note_id)
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail={
                    "message": str(e),
                    "current_revision": current_note,
                },
            ) from e
        except FileNotFoundError:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=str(e),
            ) from e
    except Exception as e:
        logger.exception("Failed to update note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/workspaces/{workspace_id}/notes/{note_id}")
async def delete_note_endpoint(
    workspace_id: str,
    note_id: str,
) -> dict[str, str]:
    """Tombstone (soft delete) a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        delete_note(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to delete note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"id": note_id, "status": "deleted"}


@router.get("/workspaces/{workspace_id}/notes/{note_id}/history")
async def get_note_history_endpoint(
    workspace_id: str,
    note_id: str,
) -> dict[str, Any]:
    """Get the revision history for a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        return get_note_history(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get note history")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/notes/{note_id}/history/{revision_id}")
async def get_note_revision_endpoint(
    workspace_id: str,
    note_id: str,
    revision_id: str,
) -> dict[str, Any]:
    """Get a specific revision of a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        return get_note_revision(ws_path, note_id, revision_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get note revision")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces/{workspace_id}/notes/{note_id}/restore")
async def restore_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteRestore,
) -> dict[str, Any]:
    """Restore a note to a previous revision."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        return restore_note(ws_path, note_id, payload.revision_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to restore note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        # ieapp.query expects workspace_path as string or Path
        return ieapp.query(str(ws_path), payload.filter)
    except Exception as e:
        logger.exception("Query failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
